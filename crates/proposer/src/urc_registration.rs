use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::LocalSigner;
use alloy::sol_types::SolCall;
use cb_common::types::BlsPublicKey;
use commit_boost::prelude::StartCommitModuleConfig;
use common::signer::call_bls_signer;
use common::types::urc::{Registration, SignedRegistration, URCRegisterInputs};
use eyre::{Context, Result};
use tracing::info;

use crate::utils::get_consensus_keys;
use urc::registry::Registry::addCollateralCall as SolAddCollateralCall;
use urc::registry::Registry::claimCollateralCall as SolClaimCollateralCall;
use urc::registry::Registry::claimSlashedCollateralCall as SolClaimSlashedCollateralCall;
use urc::registry::Registry::optInToSlasherCall as SolOptInToSlasherCall;
use urc::registry::Registry::optOutOfSlasherCall as SolOptOutOfSlasherCall;
use urc::registry::Registry::unregisterCall as SolUnregisterCall;

pub async fn sign_registration<T>(
	commit_config: &mut StartCommitModuleConfig<T>,
	pubkey: &BlsPublicKey,
	owner: Address,
	module_signing_id: &B256,
) -> Result<SignedRegistration> {
	// 1. Create Registration message
	let registration = Registration { owner };

	// 2. Get message hash
	let message_hash = registration.to_message_hash()?;

	// 3. Sign with BLS using call_bls_signer
	let bls_response = call_bls_signer(commit_config, message_hash, pubkey.clone(), module_signing_id).await?;

	// 4. Return SignedRegistration
	Ok(SignedRegistration { pubkey: pubkey.clone(), signature: bls_response.signature, nonce: bls_response.nonce })
}

/// Generate SignedRegistration for all consensus keys
pub async fn sign_registrations<T>(
	commit_config: &mut StartCommitModuleConfig<T>,
	owner: Address,
	module_signing_id: &B256,
) -> Result<URCRegisterInputs> {
	// 1. Get all consensus keys using existing get_consensus_keys()
	let pubkeys = get_consensus_keys(commit_config).await?;

	info!("Signing registrations for {} consensus keys", pubkeys.len());

	// 2. Sign registration for each key
	let mut registrations = Vec::new();
	for (i, pubkey) in pubkeys.iter().enumerate() {
		info!("  [{}/{}] Signing for pubkey: {}", i + 1, pubkeys.len(), pubkey.as_hex_string());
		let signed_reg = sign_registration(commit_config, pubkey, owner, module_signing_id).await?;
		registrations.push(signed_reg);
	}

	// 3. Return URCRegisterInputs
	Ok(URCRegisterInputs { registrations, owner, signing_id: *module_signing_id })
}

/// Send URC registration transaction
pub async fn send_registration_transaction(
	inputs: &URCRegisterInputs,
	urc_address: Address,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
	collateral: U256,
) -> Result<B256> {
	// 1. Load keystore and create signer
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;

	let wallet = EthereumWallet::from(signer);

	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());

	// 2. Create provider with wallet
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);

	// 3. Encode calldata for register()
	let calldata = inputs.abi_encode_with_selector()?;

	// 4. Create transaction
	let tx = TransactionRequest::default().with_to(urc_address).with_value(collateral).with_input(calldata);

	info!("Sending URC registration transaction...");
	info!("  URC contract: {:?}", urc_address);
	info!("  Collateral: {} wei", collateral);
	info!("  Number of keys: {}", inputs.registrations.len());

	// 5. Send transaction and wait for receipt
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;

	info!("Transaction sent, waiting for confirmation...");

	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;

	info!("URC registration transaction confirmed: {:?}", tx_hash);

	Ok(tx_hash)
}

/// Send URC optInToSlasher(registrationRoot, slasher, committer)
pub async fn send_opt_in_to_slasher_transaction(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
	committer: Address,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	// 1. Load keystore and create signer
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;

	let wallet = EthereumWallet::from(signer);

	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());

	// 2. Create provider with wallet
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);

	// 3. Encode calldata for optInToSlasher()
	let call = SolOptInToSlasherCall { registrationRoot: registration_root, slasher, committer };
	let calldata = Bytes::from(call.abi_encode());

	// 4. Create transaction
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata);

	info!(
        "Sending URC optInToSlasher transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}\n  slasher: {:?}\n  committer: {:?}",
        urc_address, registration_root, slasher, committer
    );

	// 5. Send transaction and wait for receipt
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;

	info!("Transaction sent, waiting for confirmation...");

	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;

	info!("URC optInToSlasher transaction confirmed: {:?}", tx_hash);

	Ok(tx_hash)
}

/// Send URC optOutOfSlasher(registrationRoot, slasher)
pub async fn send_opt_out_of_slasher_transaction(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	// 1. Load keystore and create signer
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;

	let wallet = EthereumWallet::from(signer);

	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());

	// 2. Create provider with wallet
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);

	// 3. Encode calldata for optOutOfSlasher()
	let call = SolOptOutOfSlasherCall { registrationRoot: registration_root, slasher };
	let calldata = Bytes::from(call.abi_encode());

	// 4. Create transaction
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata);

	info!(
		"Sending URC optOutOfSlasher transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}\n  slasher: {:?}",
		urc_address, registration_root, slasher
	);

	// 5. Send transaction and wait for receipt
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;

	info!("Transaction sent, waiting for confirmation...");

	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;

	info!("URC optOutOfSlasher transaction confirmed: {:?}", tx_hash);

	Ok(tx_hash)
}

/// Send URC unregister(registrationRoot)
pub async fn send_unregister_transaction(
	urc_address: Address,
	registration_root: B256,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;
	let wallet = EthereumWallet::from(signer);
	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);
	let call = SolUnregisterCall { registrationRoot: registration_root };
	let calldata = Bytes::from(call.abi_encode());
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata);
	info!(
		"Sending URC unregister transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}",
		urc_address, registration_root
	);
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;
	info!("Transaction sent, waiting for confirmation...");
	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;
	info!("URC unregister transaction confirmed: {:?}", tx_hash);
	Ok(tx_hash)
}

/// Send URC addCollateral(registrationRoot), attaching ETH value
pub async fn send_add_collateral_transaction(
	urc_address: Address,
	registration_root: B256,
	amount_wei: U256,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;
	let wallet = EthereumWallet::from(signer);
	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);
	let call = SolAddCollateralCall { registrationRoot: registration_root };
	let calldata = Bytes::from(call.abi_encode());
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata).with_value(amount_wei);
	info!(
		"Sending URC addCollateral transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}\n  amount: {} wei",
		urc_address, registration_root, amount_wei
	);
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;
	info!("Transaction sent, waiting for confirmation...");
	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;
	info!("URC addCollateral transaction confirmed: {:?}", tx_hash);
	Ok(tx_hash)
}

/// Read URC getSlasherCommitment(registrationRoot, slasher)
pub async fn get_slasher_commitment(
	urc_address: Address,
	registration_root: B256,
	slasher: Address,
	execution_rpc_url: &str,
) -> Result<urc::registry::IRegistry::SlasherCommitment> {
	let provider = ProviderBuilder::new().connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);
	let registry = urc::registry::Registry::new(urc_address, provider);
	let sc = registry.getSlasherCommitment(registration_root, slasher).call().await?;
	Ok(sc)
}

/// Send URC claimCollateral(registrationRoot)
pub async fn send_claim_collateral_transaction(
	urc_address: Address,
	registration_root: B256,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;
	let wallet = EthereumWallet::from(signer);
	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);
	let call = SolClaimCollateralCall { registrationRoot: registration_root };
	let calldata = Bytes::from(call.abi_encode());
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata);
	info!(
		"Sending URC claimCollateral transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}",
		urc_address, registration_root
	);
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;
	info!("Transaction sent, waiting for confirmation...");
	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;
	info!("URC claimCollateral transaction confirmed: {:?}", tx_hash);
	Ok(tx_hash)
}

/// Send URC claimSlashedCollateral(registrationRoot)
pub async fn send_claim_slashed_collateral_transaction(
	urc_address: Address,
	registration_root: B256,
	execution_rpc_url: &str,
	keystore_path: &str,
	password: &str,
) -> Result<B256> {
	let signer = LocalSigner::decrypt_keystore(keystore_path, password)
		.context("Failed to decrypt keystore")?;
	let wallet = EthereumWallet::from(signer);
	info!("Loaded wallet from keystore: {:?}", wallet.default_signer().address());
	let provider =
		ProviderBuilder::new().wallet(wallet).connect_http(execution_rpc_url.parse().context("Invalid RPC URL")?);
	let call = SolClaimSlashedCollateralCall { registrationRoot: registration_root };
	let calldata = Bytes::from(call.abi_encode());
	let tx = TransactionRequest::default().with_to(urc_address).with_input(calldata);
	info!(
		"Sending URC claimSlashedCollateral transaction...\n  URC contract: {:?}\n  registrationRoot: {:?}",
		urc_address, registration_root
	);
	let pending_tx = provider.send_transaction(tx).await.context("Failed to send transaction")?;
	info!("Transaction sent, waiting for confirmation...");
	let tx_hash = pending_tx.watch().await.context("Failed to confirm transaction")?;
	info!("URC claimSlashedCollateral transaction confirmed: {:?}", tx_hash);
	Ok(tx_hash)
}
