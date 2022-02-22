// Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_PUBLIC_KEYS: &[u8] = b"dkg-metadata::public_key";

// Lock Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_PUBLIC_KEYS_LOCK: &[u8] = b"dkg-metadata::public_key::lock";

// Key for offchain storage of aggregated derived public keys for genesis authorities
pub const AGGREGATED_PUBLIC_KEYS_AT_GENESIS: &[u8] = b"dkg-metadata::genesis_public_keys";

// Lock Key for offchain storage of aggregated derived public keys for genesis authorities
pub const AGGREGATED_PUBLIC_KEYS_AT_GENESIS_LOCK: &[u8] =
	b"dkg-metadata::genesis_public_keys::lock";

// Key for offchain storage of derived public key
pub const SUBMIT_KEYS_AT: &[u8] = b"dkg-metadata::submit_keys_at";

// Key for offchain storage of derived public key
pub const SUBMIT_GENESIS_KEYS_AT: &[u8] = b"dkg-metadata::submit_genesis_keys_at";

// Key for offchain storage of derived public key signature
pub const OFFCHAIN_PUBLIC_KEY_SIG: &[u8] = b"dkg-metadata::public_key_sig";

// Lock Key for offchain storage of derived public key signature
pub const OFFCHAIN_PUBLIC_KEY_SIG_LOCK: &[u8] = b"dkg-metadata::public_key_sig::lock";

// Key for offchain signed proposals storage
pub const OFFCHAIN_SIGNED_PROPOSALS: &[u8] = b"dkg-proposal-handler::signed_proposals";

// Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_MISBEHAVIOUR_REPORTS: &[u8] = b"dkg-metadata::misbehaviour";

// Lock Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_MISBEHAVIOUR_REPORTS_LOCK: &[u8] = b"dkg-metadata::misbehaviour::lock";

// Lock Key for submitting signed proposal on chain
pub const SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK: &[u8] =
	b"dkg-proposal-handler::submit_signed_proposal_on_chain::lock";
