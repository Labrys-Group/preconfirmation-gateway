//! Proposer CLI: argument parsing and subcommand handlers.

use alloy::primitives::{Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::BlockNumberOrTag;
use clap::{Parser, Subcommand};
use commit_boost::prelude::*;
use common::beacon::{BeaconApiClient, ReqwestClient};
use common::config::BeaconApiConfig;
use common::slot_timer::SlotTimer;
use constraints::ConstraintsClient;
use eyre::{Context, Result};
use tracing::{debug, error, info};

/// CLI for the proposer service.
#[derive(Parser)]
#[command(name = "proposer")]
#[command(about = "Proposer service for preconfirmations", long_about = None)]
pub struct Cli {
	#[command(subcommand)]
	pub command: Option<Commands>,
}

/// Supported subcommands for the proposer.
#[derive(Subcommand)]
pub enum Commands {
	/// Run the proposer service daemon
	Run,

	/// Register BLS keys with the URC contract
	Register {
		/// URC contract address
		#[arg(long)]
		urc_address: Address,

		/// Path to keystore file
		#[arg(long)]
		keystore: String,

		/// Keystore password (will prompt if not provided)
		#[arg(long)]
		password: Option<String>,

		/// Dry run - sign registrations but don't send transaction
		#[arg(long)]
		dry_run: bool,
	},

	/// Opt-in to a committer for a given slasher
	OptInToSlasher {
		/// URC contract address
		#[arg(long)]
		urc_address: Address,

		/// Registration root to operate on
		#[arg(long)]
		registration_root: B256,

		/// Slasher address
		#[arg(long)]
		slasher: Address,

		/// Committer address to authorize
		#[arg(long)]
		committer: Address,

		/// Path to keystore file
		#[arg(long)]
		keystore: String,

		/// Keystore password (will prompt if not provided)
		#[arg(long)]
		password: Option<String>,

		/// Skip pre-flight checks
		#[arg(long)]
		skip_precheck: bool,
	},

	/// Opt-out of a given slasher
	OptOutOfSlasher {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		slasher: Address,
		#[arg(long)]
		keystore: String,
		#[arg(long)]
		password: Option<String>,
	},

	/// Unregister an operator by registration root
	Unregister {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		keystore: String,
		#[arg(long)]
		password: Option<String>,
	},

	/// Add collateral (in wei) to an existing operator
	AddCollateral {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		amount_wei: U256,
		#[arg(long)]
		keystore: String,
		#[arg(long)]
		password: Option<String>,
	},

	/// Get the slasher commitment for a registration+slasher
	GetSlasherCommitment {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		slasher: Address,
	},

	/// Claim collateral after unregister and unregistrationDelay elapsed
	ClaimCollateral {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		keystore: String,
		#[arg(long)]
		password: Option<String>,
	},

	/// Claim slashed collateral after slashWindow elapsed
	ClaimSlashedCollateral {
		#[arg(long)]
		urc_address: Address,
		#[arg(long)]
		registration_root: B256,
		#[arg(long)]
		keystore: String,
		#[arg(long)]
		password: Option<String>,
	},
}

async fn handle_opt_in_to_slasher_command(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
	committer: Address,
	keystore_path: String,
	password: Option<String>,
	skip_precheck: bool,
) -> Result<()> {
	info!("Starting URC opt-in to slasher process");

	// Load configuration
	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	// Get password
	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	// Build wallet+provider to use for both checks and sending
	let signer = alloy::signers::local::LocalSigner::decrypt_keystore(&keystore_path, &password)
		.context("Failed to decrypt keystore")?;
	let wallet = alloy::network::EthereumWallet::from(signer);
	let provider = ProviderBuilder::new()
		.wallet(wallet.clone())
		.connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);

	// Pre-flight checks
	if !skip_precheck {
		info!("Running pre-flight checks...");

		// Create registry instance for view calls
		let registry = urc::registry::Registry::new(urc_address, provider.clone());

		// Owner match
		let owner_resp = registry.getOperatorData(registration_root).call().await.context("getOperatorData failed")?;
		let owner = owner_resp.owner;
		let sender_addr = wallet.default_signer().address();
		if owner != sender_addr {
			return Err(eyre::eyre!("Owner mismatch: on-chain {:?} != sender {:?}", owner, sender_addr));
		}

		// Fraud window elapsed
		let cfg = registry.getConfig().call().await.context("getConfig failed")?;
		let registered_at: u64 = owner_resp.registeredAt.to();
		let fraud_window: u64 = cfg.fraudProofWindow as u64;

		// Fetch latest block timestamp via eth_getBlockByNumber
		let latest =
			provider.get_block_by_number(BlockNumberOrTag::Latest).await.context("Failed to fetch latest block")?;
		if let Some(latest_block) = latest {
			let latest_ts = latest_block.header.timestamp;
			if latest_ts < registered_at + fraud_window {
				return Err(eyre::eyre!("Fraud window not met: now {} < {}", latest_ts, registered_at + fraud_window));
			}
		}

		// Not already opted in
		if registry.isOptedIntoSlasher(registration_root, slasher).call().await.context("isOptedIntoSlasher failed")? {
			return Err(eyre::eyre!("Already opted into this slasher"));
		}

		// Not slashed
		if registry.isSlashed_0(registration_root).call().await.context("isSlashed(regRoot) failed")? {
			return Err(eyre::eyre!("Operator already slashed"));
		}
	}

	// Send tx
	let tx_hash = crate::send_opt_in_to_slasher_transaction(
		urc_address,
		registration_root,
		slasher,
		committer,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send optInToSlasher transaction")?;

	info!("✅ Opt-in successful!\nTransaction hash: {:?}", tx_hash);

	Ok(())
}

/// Entry point for running the CLI; parses args and dispatches subcommands.
pub async fn run() -> Result<()> {
	let cli = Cli::parse();
	match cli.command.unwrap_or(Commands::Run) {
		Commands::Run => run_daemon().await,
		command => handle_command(command).await,
	}
}

/// Handle non-daemon CLI commands
pub async fn handle_command(command: Commands) -> Result<()> {
	match command {
		Commands::Run => {
			unreachable!("Run command should be handled separately")
		}
		Commands::Register { urc_address, keystore, password, dry_run } => {
			handle_register_command(urc_address, keystore, password, dry_run).await
		}
		Commands::OptInToSlasher {
			urc_address,
			registration_root,
			slasher,
			committer,
			keystore,
			password,
			skip_precheck,
		} => {
			handle_opt_in_to_slasher_command(
				urc_address,
				registration_root,
				slasher,
				committer,
				keystore,
				password,
				skip_precheck,
			)
			.await
		}
		Commands::OptOutOfSlasher { urc_address, registration_root, slasher, keystore, password } => {
			handle_opt_out_of_slasher_command(urc_address, registration_root, slasher, keystore, password).await
		}
		Commands::Unregister { urc_address, registration_root, keystore, password } => {
			handle_unregister_command(urc_address, registration_root, keystore, password).await
		}
		Commands::AddCollateral { urc_address, registration_root, amount_wei, keystore, password } => {
			handle_add_collateral_command(urc_address, registration_root, amount_wei, keystore, password).await
		}
		Commands::GetSlasherCommitment { urc_address, registration_root, slasher } => {
			handle_get_slasher_commitment_command(urc_address, registration_root, slasher).await
		}
		Commands::ClaimCollateral { urc_address, registration_root, keystore, password } => {
			handle_claim_collateral_command(urc_address, registration_root, keystore, password).await
		}
		Commands::ClaimSlashedCollateral { urc_address, registration_root, keystore, password } => {
			handle_claim_slashed_collateral_command(urc_address, registration_root, keystore, password).await
		}
	}
}

/// Configuration and state for daemon work loop
pub struct DaemonContext {
	pub commit_config: StartCommitModuleConfig<crate::ProposerConfig>,
	pub beacon_client: BeaconApiClient<ReqwestClient>,
	pub constraints_client: ConstraintsClient,
	pub slot_timer: SlotTimer,
	pub poll_interval_seconds: u64,
	pub database: common::types::database::DatabaseContext,
}

/// Load daemon configuration (must be called from main async context)
pub async fn load_daemon_config() -> Result<DaemonContext> {
	// Load configuration using commit-boost's config loader
	// CRITICAL: This must be called in main async context, not in spawned task
	let commit_config = load_commit_module_config::<crate::ProposerConfig>()
		.map_err(|e| eyre::eyre!("Failed to load commit module config: {}", e))?;

	info!("Loaded config");

	let config = commit_config.extra.clone();

	info!("Proposer configuration:");
	info!("  Delegate BLS key: {}", config.delegate_bls_public_key);
	info!("  Committer address: {}", config.committer_address);
	info!("  Relay URL: {}", config.relay_url);
	info!("  Beacon API URL: {}", config.beacon_api_url);
	info!("  Poll interval: {} seconds", config.poll_interval_seconds);

	// Create beacon API client
	let beacon_config = BeaconApiConfig {
		primary_endpoint: config.beacon_api_url.clone(),
		fallback_endpoints: vec![],
		request_timeout_secs: 30,
		genesis_time: config.beacon_genesis_timestamp,
	};

	let beacon_client = BeaconApiClient::with_default_client(beacon_config)
		.map_err(|e| eyre::eyre!("Failed to create beacon API client: {}", e))?;

	info!("Beacon API client initialized");

	// Create constraints client for posting delegations to relay
	let constraints_client = ConstraintsClient::new(config.relay_url.clone(), config.relay_api_key.clone());

	// Test relay connectivity
	match constraints_client.health_check().await {
		Ok(true) => info!("Relay health check passed"),
		Ok(false) => error!("Relay health check failed"),
		Err(e) => error!("Failed to check relay health: {}", e),
	}

	// Create slot timer for tracking current slot
	let slot_timer = SlotTimer::new(config.beacon_genesis_timestamp);
	let poll_interval_seconds = config.poll_interval_seconds;

	// Initialize RocksDB for delegation storage (equivocation prevention)
	let db_path = &config.delegation_db_path;
	info!("Initializing delegation database at: {}", db_path);

	let mut db_opts = rocksdb::Options::default();
	db_opts.create_if_missing(true);
	let db = rocksdb::DB::open(&db_opts, db_path)
		.map_err(|e| eyre::eyre!("Failed to open delegation database at {}: {}", db_path, e))?;
	let database = common::types::database::DatabaseContext::new(std::sync::Arc::new(db));

	info!("Delegation database initialized successfully");

	Ok(DaemonContext { commit_config, beacon_client, constraints_client, slot_timer, poll_interval_seconds, database })
}

/// Run the daemon work loop (can be called from spawned task)
pub async fn run_daemon_loop(mut context: DaemonContext) -> Result<()> {
	info!("Starting proposer duty polling loop");

	// Set up polling interval
	let mut poll_interval = tokio::time::interval(std::time::Duration::from_secs(context.poll_interval_seconds));

	loop {
		poll_interval.tick().await;

		let current_slot = context.slot_timer.get_current_slot();
		debug!("Checking proposer duties for current slot: {}", current_slot);

		// Process lookahead to find and post delegations
		match crate::process_lookahead(
			&context.beacon_client,
			&context.constraints_client,
			&mut context.commit_config,
			current_slot,
			&context.database,
		)
		.await
		{
			Ok(_) => {
				debug!("Lookahead processed successfully for slot {}", current_slot);
			}
			Err(e) => {
				error!("Error processing lookahead for slot {}: {}", current_slot, e);
			}
		}
	}
}

/// Legacy function kept for backwards compatibility
pub async fn run_daemon() -> Result<()> {
	let context = load_daemon_config().await?;
	run_daemon_loop(context).await
}

async fn handle_register_command(
	urc_address: Address,
	keystore_path: String,
	password: Option<String>,
	dry_run: bool,
) -> Result<()> {
	info!("Starting URC registration process");

	// Load configuration
	let mut commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;

	// Clone config fields we'll need later to avoid borrow checker issues
	let owner: Address = commit_config.extra.urc_owner.parse().context("Invalid URC owner address")?;
	let module_signing_id: B256 = commit_config.extra.module_signing_id.parse().context("Invalid module signing ID")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();
	let collateral: U256 =
		commit_config.extra.registration_collateral_wei.parse().context("Invalid collateral amount")?;

	info!("URC Registration Parameters:");
	info!("  URC Contract: {:?}", urc_address);
	info!("  Owner: {:?}", owner);
	info!("  Execution RPC: {}", execution_rpc_url);
	info!("  Collateral: {} wei", collateral);

	// Sign registrations for all keys
	info!("Signing registrations for all consensus keys...");
	let inputs = crate::sign_registrations(&mut commit_config, owner, &module_signing_id)
		.await
		.context("Failed to sign registrations")?;

	info!("Successfully signed {} registrations", inputs.registrations.len());
	for (i, reg) in inputs.registrations.iter().enumerate() {
		info!("  [{}] Pubkey: {}", i + 1, reg.pubkey.as_hex_string());
		info!("       Nonce:  {}", reg.nonce);
	}

	if dry_run {
		info!("Dry run mode - not sending transaction");
		info!("Would register {} keys to URC at {:?}", inputs.registrations.len(), urc_address);
		return Ok(());
	}

	// Get password
	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	// Send registration transaction
	info!("Sending URC registration transaction...");
	let tx_hash = crate::send_registration_transaction(
		&inputs,
		urc_address,
		&execution_rpc_url,
		&keystore_path,
		&password,
		collateral,
	)
	.await
	.context("Failed to send registration transaction")?;

	info!("✅ Registration successful!");
	info!("Transaction hash: {:?}", tx_hash);
	info!("Owner: {:?}", owner);
	info!("Number of keys registered: {}", inputs.registrations.len());

	Ok(())
}

async fn handle_opt_out_of_slasher_command(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
	keystore_path: String,
	password: Option<String>,
) -> Result<()> {
	info!("Starting URC opt-out of slasher process");

	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	let tx_hash = crate::send_opt_out_of_slasher_transaction(
		urc_address,
		registration_root,
		slasher,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send optOutOfSlasher transaction")?;

	info!("✅ Opt-out successful!\nTransaction hash: {:?}", tx_hash);
	Ok(())
}

async fn handle_unregister_command(
	urc_address: Address,
	registration_root: B256,
	keystore_path: String,
	password: Option<String>,
) -> Result<()> {
	info!("Starting URC unregister process");

	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	let tx_hash = crate::send_unregister_transaction(
		urc_address,
		registration_root,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send unregister transaction")?;

	info!("✅ Unregister successful!\nTransaction hash: {:?}", tx_hash);
	Ok(())
}

async fn handle_add_collateral_command(
	urc_address: Address,
	registration_root: B256,
	amount_wei: U256,
	keystore_path: String,
	password: Option<String>,
) -> Result<()> {
	info!("Starting URC add-collateral process");

	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	let tx_hash = crate::send_add_collateral_transaction(
		urc_address,
		registration_root,
		amount_wei,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send addCollateral transaction")?;

	info!("✅ Add-collateral successful!\nTransaction hash: {:?}", tx_hash);
	Ok(())
}

async fn handle_get_slasher_commitment_command(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
) -> Result<()> {
	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let sc = crate::get_slasher_commitment(urc_address, registration_root, slasher, &execution_rpc_url)
		.await
		.context("Failed to fetch slasher commitment")?;

	info!(
		"SlasherCommitment: optedInAt={}, optedOutAt={}, slashed={}, committer={:?}",
		sc.optedInAt, sc.optedOutAt, sc.slashed, sc.committer
	);
	Ok(())
}

async fn handle_claim_collateral_command(
	urc_address: Address,
	registration_root: B256,
	keystore_path: String,
	password: Option<String>,
) -> Result<()> {
	info!("Starting URC claim-collateral process");

	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	let tx_hash = crate::send_claim_collateral_transaction(
		urc_address,
		registration_root,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send claimCollateral transaction")?;

	info!("✅ Claim-collateral successful!\nTransaction hash: {:?}", tx_hash);
	Ok(())
}

async fn handle_claim_slashed_collateral_command(
	urc_address: Address,
	registration_root: B256,
	keystore_path: String,
	password: Option<String>,
) -> Result<()> {
	info!("Starting URC claim-slashed-collateral process");

	let commit_config =
		load_commit_module_config::<crate::ProposerConfig>().context("Failed to load commit module config")?;
	let execution_rpc_url = commit_config.extra.execution_rpc_url.clone();

	let password = match password {
		Some(p) => p,
		None => {
			info!("Enter keystore password:");
			rpassword::read_password().context("Failed to read password")?
		}
	};

	let tx_hash = crate::send_claim_slashed_collateral_transaction(
		urc_address,
		registration_root,
		&execution_rpc_url,
		&keystore_path,
		&password,
	)
	.await
	.context("Failed to send claimSlashedCollateral transaction")?;

	info!("✅ Claim-slashed-collateral successful!\nTransaction hash: {:?}", tx_hash);
	Ok(())
}
