#![cfg_attr(not(feature = "std"), no_std)]
// NOTE: needed to silence warnings about generated code in `decl_runtime_apis`
#![allow(clippy::too_many_arguments, clippy::unnecessary_mut_passed)]

pub mod mmr;
pub mod proposal;
pub mod traits;
pub mod utils;

pub use ethereum::*;
pub use ethereum_types::*;
use frame_support::RuntimeDebug;
pub use proposal::*;

pub use crate::proposal::DKGPayloadKey;
use codec::{Codec, Decode, Encode};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{
	traits::{IdentifyAccount, Verify},
	MultiSignature,
};
use sp_std::{collections::vec_deque::VecDeque, prelude::*, vec::Vec};
use tiny_keccak::{Hasher, Keccak};

/// Utility fn to calculate keccak 256 has
pub fn keccak_256(data: &[u8]) -> [u8; 32] {
	let mut keccak = Keccak::v256();
	keccak.update(data);
	let mut output = [0u8; 32];
	keccak.finalize(&mut output);
	output
}

/// The type used to represent an MMR root hash.
pub type MmrRootHash = H256;

/// Authority set id starts with zero at genesis
pub const GENESIS_AUTHORITY_SET_ID: u64 = 0;

// Engine ID for DKG
pub const DKG_ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

// Key type for DKG keys
pub const KEY_TYPE: sp_application_crypto::KeyTypeId = sp_application_crypto::KeyTypeId(*b"wdkg");

// Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_PUBLIC_KEYS: &[u8] = b"dkg-metadata::public_key";

// Key for offchain storage of aggregated derived public keys for genesis authorities
pub const AGGREGATED_PUBLIC_KEYS_AT_GENESIS: &[u8] = b"dkg-metadata::genesis_public_keys";

// Key for offchain storage of derived public key
pub const SUBMIT_KEYS_AT: &[u8] = b"dkg-metadata::submit_keys_at";

// Key for offchain storage of derived public key
pub const SUBMIT_GENESIS_KEYS_AT: &[u8] = b"dkg-metadata::submit_genesis_keys_at";

// Key for offchain storage of derived public key signature
pub const OFFCHAIN_PUBLIC_KEY_SIG: &[u8] = b"dkg-metadata::public_key_sig";
// Key for offchain signed proposals storage
pub const OFFCHAIN_SIGNED_PROPOSALS: &[u8] = b"dkg-proposal-handler::signed_proposals";

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct OffchainSignedProposals {
	pub proposals: VecDeque<ProposalType>,
}

pub type PublicKeyAndSignature = (Vec<u8>, Vec<u8>);

#[derive(Eq, PartialEq, Clone, Encode, Default, Decode, RuntimeDebug, TypeInfo)]
pub struct AggregatedPublicKeys {
	/// A vector of public keys and signature pairs [/public_key/] , [/signature/]
	pub keys_and_signatures: Vec<PublicKeyAndSignature>,
}

impl Default for OffchainSignedProposals {
	fn default() -> Self {
		Self { proposals: VecDeque::default() }
	}
}

pub mod offchain_crypto {
	use sp_core::sr25519::Signature as Sr25519Signature;
	use sp_runtime::{
		app_crypto::{app_crypto, sr25519},
		key_types::ACCOUNT,
		traits::Verify,
		MultiSignature, MultiSigner,
	};
	app_crypto!(sr25519, ACCOUNT);

	pub struct OffchainAuthId;

	impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for OffchainAuthId {
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}

	impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature>
		for OffchainAuthId
	{
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}
}

pub mod crypto {
	use sp_application_crypto::{app_crypto, ecdsa};
	use sp_runtime::{traits::Verify, MultiSignature, MultiSigner};
	app_crypto!(ecdsa, crate::KEY_TYPE);

	/// Identity of a DKG authority using ECDSA as its crypto.
	pub type AuthorityId = Public;

	/// Signature for a DKG authority using ECDSA as its crypto.
	pub type AuthoritySignature = Signature;
}

pub type AuthoritySetId = u64;

#[derive(Decode, Encode, Default, Debug, PartialEq, Clone, TypeInfo)]
pub struct AuthoritySet<AuthorityId> {
	/// Public keys of the validator set elements
	pub authorities: Vec<AuthorityId>,
	/// Identifier of the validator set
	pub id: AuthoritySetId,
}

impl<AuthorityId> AuthoritySet<AuthorityId> {
	/// Return an empty validator set with id of 0.
	pub fn empty() -> Self {
		Self { authorities: Default::default(), id: Default::default() }
	}
}

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct Commitment<TBlockNumber, TPayload> {
	pub payload: TPayload,
	pub block_number: TBlockNumber,
	pub validator_set_id: AuthoritySetId,
}

pub type AuthorityIndex = u32;

#[derive(Decode, Encode)]
pub enum ConsensusLog<AuthorityId: Codec> {
	/// The authorities have changed.
	#[codec(index = 1)]
	AuthoritiesChange {
		next_authorities: AuthoritySet<AuthorityId>,
		next_queued_authorities: AuthoritySet<AuthorityId>,
	},
	/// Disable the authority with given index.
	#[codec(index = 2)]
	OnDisabled(AuthorityIndex),
	/// MMR root hash.
	#[codec(index = 3)]
	MmrRoot(MmrRootHash),
	/// The authority keys have changed
	#[codec(index = 4)]
	KeyRefresh { old_public_key: Vec<u8>, new_public_key: Vec<u8>, new_key_signature: Vec<u8> },
}

type AccountId = <<MultiSignature as Verify>::Signer as IdentifyAccount>::AccountId;

sp_api::decl_runtime_apis! {

	pub trait DKGApi<AuthorityId, BlockNumber> where
		AuthorityId: Codec + PartialEq,
		BlockNumber: Codec + PartialEq + sp_runtime::traits::AtLeast32BitUnsigned
	{
		/// Return the current active authority set
		fn authority_set() -> AuthoritySet<AuthorityId>;
		/// Return the current signature threshold for the DKG
		fn signature_threshold() -> u16;
		/// Return the next authorities active authority set
		fn queued_authority_set() -> AuthoritySet<AuthorityId>;
		/// Check if refresh process should start
		fn should_refresh(_block_number: BlockNumber) -> bool;
		/// Fetch DKG public key for queued authorities
		fn next_dkg_pub_key() -> Option<Vec<u8>>;
		/// Fetch DKG public key for current authorities
		fn dkg_pub_key() -> Option<Vec<u8>>;
		/// Get list of unsigned proposals
		fn get_unsigned_proposals() -> Vec<(DKGPayloadKey, ProposalType)>;
		/// Get maximum delay before which an offchain extrinsic should be submitted
		fn get_max_extrinsic_delay(_block_number: BlockNumber) -> BlockNumber;
		/// Current and Queued Authority Account Ids [/current_authorities/, /next_authorities/]
		fn get_authority_accounts() -> (Vec<AccountId>, Vec<AccountId>);
		/// Fetch DKG public key for sig
		fn next_pub_key_sig() -> Option<Vec<u8>>;
	}
}
