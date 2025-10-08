pub mod api;
pub mod config;
pub mod crypto;
pub mod db;
pub mod metrics;
pub mod rpc;
pub mod server;
pub mod services;
pub mod testing;
pub mod types;
pub mod utils;

// Re-export commonly used types and functions for easier access
pub use config::{Config, ValidationConfig};
pub use db::{DatabaseContext, create_pool, test_connection};
pub use rpc::handlers::{commitment_request_handler, commitment_result_handler, fee_handler, slots_handler};
pub use types::{Commitment, CommitmentRequest, FeeInfo, RpcContext, SignedCommitment, SlotInfoResponse};
