// Bridge proposals
pub mod resource_id_update;

// Anchor proposals
pub mod anchor_update;
pub mod set_verifier;

// Token update proposals
pub mod add_token_to_set;
pub mod fee_update;
pub mod remove_token_from_set;

// Treasury Proposals
pub mod fee_recipient_update;
pub mod set_treasury_handler;

// Rescue tokens
pub mod rescue_tokens;

// fees & limits
pub mod max_deposit_limit_update;
pub mod min_withdrawal_limit_update;

// Generic proposals
pub mod bytes32_update;

// EVM tx
pub mod evm_tx;
