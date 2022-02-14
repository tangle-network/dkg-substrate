#![cfg_attr(not(feature = "std"), no_std)]
// NOTE: needed to silence warnings about generated code in `decl_runtime_apis`
#![allow(clippy::too_many_arguments, clippy::unnecessary_mut_passed)]

pub mod handlers;
pub mod mmr;
pub mod offchain;
pub mod proposal;
pub mod traits;
pub mod utils;

use crypto::AuthorityId;
pub use ethereum::*;
pub use ethereum_types::*;
use frame_support::RuntimeDebug;
pub use proposal::*;
use sp_application_crypto::sr25519;

pub use crate::proposal::DKGPayloadKey;
use codec::{Codec, Decode, Encode};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{
	create_runtime_str,
	traits::{AtLeast32Bit, IdentifyAccount, Verify},
	MultiSignature, RuntimeString,
};
use sp_std::{prelude::*, vec::Vec};
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

pub type ChainId = u32;

/// Authority set id starts with zero at genesis
pub const GENESIS_AUTHORITY_SET_ID: u64 = 0;

pub const GENESIS_BLOCK_NUMBER: u32 = 0;

// Engine ID for DKG
pub const DKG_ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

// Key type for DKG keys
pub const KEY_TYPE: sp_application_crypto::KeyTypeId = sp_application_crypto::KeyTypeId(*b"wdkg");

// Untrack interval for unsigned proposals completed stages for signing
pub const UNTRACK_INTERVAL: u32 = 10;

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct OffchainSignedProposals<BlockNumber> {
	pub proposals: Vec<(Vec<Proposal>, BlockNumber)>,
}

pub type PublicKeyAndSignature = (Vec<u8>, Vec<u8>);

#[derive(Eq, PartialEq, Clone, Encode, Default, Decode, RuntimeDebug, TypeInfo)]
pub struct AggregatedPublicKeys {
	/// A vector of public keys and signature pairs [/public_key/] , [/signature/]
	pub keys_and_signatures: Vec<PublicKeyAndSignature>,
}

#[derive(Eq, PartialEq, Clone, Encode, Decode, Debug, TypeInfo)]
pub struct AggregatedMisbehaviorReports {
	/// The round id the offense took place in
	pub round_id: u64,
	/// The offending authority
	pub offender: crypto::AuthorityId,
	/// A list of reporters
	pub reporters: Vec<sr25519::Public>,
	/// A list of signed reports
	pub signatures: Vec<Vec<u8>>,
}

impl<BlockNumber> Default for OffchainSignedProposals<BlockNumber> {
	fn default() -> Self {
		Self { proposals: Vec::default() }
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

#[derive(Decode, Encode, Debug, PartialEq, Clone, TypeInfo)]
pub struct AuthoritySet<AuthorityId> {
	/// Public keys of the validator set elements
	pub authorities: Vec<AuthorityId>,
	/// Identifier of the validator set
	pub id: AuthoritySetId,
}

impl Default for AuthoritySet<AuthorityId> {
	fn default() -> Self {
		Self { authorities: vec![], id: Default::default() }
	}
}

impl<AuthorityId> AuthoritySet<AuthorityId> {
	/// Return an empty validator set with id of 0.
	pub fn empty() -> Self {
		Self { authorities: vec![], id: Default::default() }
	}
}

pub enum DKGReport {
	KeygenMisbehavior { offender: AuthorityId },
	SigningMisbehavior { offender: AuthorityId },
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
	/// The DKG keys have changed
	#[codec(index = 4)]
	KeyRefresh { old_public_key: Vec<u8>, new_public_key: Vec<u8>, new_key_signature: Vec<u8> },
}

#[derive(Encode, Decode, PartialEq, Eq, Clone, RuntimeDebug, scale_info::TypeInfo)]
pub enum ChainIdType<ChainId> {
	// EVM(chain_identifier)
	EVM(ChainId),
	// Substrate(chain_identifier)
	Substrate(ChainId),
	// Relay chain(relay_chain_identifier, chain_identifier)
	RelayChain(RuntimeString, ChainId),
	// Parachain(relay_chain_identifier, para_id)
	Parachain(RuntimeString, ChainId),
	// Cosmos
	CosmosSDK(ChainId),
	// Solana
	Solana(ChainId),
}

impl<ChainId: ChainIdTrait> ChainIdType<ChainId> {
	pub fn inner_id(&self) -> ChainId {
		match self {
			ChainIdType::EVM(id) => id.clone(),
			ChainIdType::Substrate(id) => id.clone(),
			ChainIdType::RelayChain(_, id) => id.clone(),
			ChainIdType::Parachain(_, id) => id.clone(),
			ChainIdType::CosmosSDK(id) => id.clone(),
			ChainIdType::Solana(id) => id.clone(),
		}
	}

	pub fn to_type(&self) -> u16 {
		match self {
			ChainIdType::EVM(_) => 1,
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) => 2,
			ChainIdType::CosmosSDK(_) => 3,
			ChainIdType::Solana(_) => 4,
		}
	}

	pub fn get_type_bytes(&self) -> [u8; 2] {
		let polkadot_str = create_runtime_str!("polkadot");
		let kusama_str = create_runtime_str!("kusama");
		match self {
			ChainIdType::EVM(_) => [1, 0],
			ChainIdType::Substrate(_) => [2, 0],
			ChainIdType::RelayChain(relay, _) =>
				if relay == &polkadot_str {
					[2, 1]
				} else if relay == &kusama_str {
					[2, 2]
				} else {
					panic!("Unknown relay chain id: {:?}", relay);
				},
			ChainIdType::Parachain(relay, _) =>
				if relay == &polkadot_str {
					[2, 128]
				} else if relay == &kusama_str {
					[2, 129]
				} else {
					panic!("Unknown relay chain id: {:?}", relay);
				},
			ChainIdType::CosmosSDK(_) => [3, 0],
			ChainIdType::Solana(_) => [4, 0],
			_ => panic!("Invalid chain id type"),
		}
	}

	pub fn from_raw(bytes: &[u8]) -> Self {
		let mut chain_type_bytes = [0u8; 2];
		let mut chain_id_bytes = [0u8; 4];
		if bytes.len() == 6 {
			chain_type_bytes.copy_from_slice(&bytes[0..2]);
			chain_id_bytes.copy_from_slice(&bytes[2..6]);
		}

		if bytes.len() == 8 {
			chain_type_bytes.copy_from_slice(&bytes[2..4]);
			chain_id_bytes.copy_from_slice(&bytes[4..8]);
		}

		Self::from_raw_parts(chain_type_bytes, chain_id_bytes)
	}

	pub fn from_raw_parts(chain_type_bytes: [u8; 2], chain_id_bytes: [u8; 4]) -> Self {
		Self::get_full_repr(chain_type_bytes, ChainId::from(u32::from_be_bytes(chain_id_bytes)))
	}

	pub fn get_full_repr(chain_type: [u8; 2], chain_id: ChainId) -> Self {
		match chain_type {
			[1, 0] => ChainIdType::EVM(ChainId::from(chain_id)),
			[2, 0] => ChainIdType::Substrate(ChainId::from(chain_id)),
			[2, 1] =>
				ChainIdType::RelayChain(create_runtime_str!("polkadot"), ChainId::from(chain_id)),
			[2, 2] =>
				ChainIdType::RelayChain(create_runtime_str!("kusama"), ChainId::from(chain_id)),
			[2, 128] =>
				ChainIdType::RelayChain(create_runtime_str!("polkadot"), ChainId::from(chain_id)),
			[2, 129] =>
				ChainIdType::Parachain(create_runtime_str!("kusama"), ChainId::from(chain_id)),
			[3, 0] => ChainIdType::CosmosSDK(ChainId::from(chain_id)),
			[4, 0] => ChainIdType::Solana(ChainId::from(chain_id)),
			_ => panic!("Invalid chain id type"),
		}
	}
}

type AccountId = <<MultiSignature as Verify>::Signer as IdentifyAccount>::AccountId;

sp_api::decl_runtime_apis! {

	pub trait DKGApi<AuthorityId, N> where
		AuthorityId: Codec + PartialEq,
		N: Codec + PartialEq + sp_runtime::traits::AtLeast32BitUnsigned,
	{
		/// Return the current active authority set
		fn authority_set() -> AuthoritySet<AuthorityId>;
		/// Return the current signature threshold for the DKG
		fn signature_threshold() -> u16;
		/// Return the next authorities active authority set
		fn queued_authority_set() -> AuthoritySet<AuthorityId>;
		/// Check if refresh process should start
		fn should_refresh(_block_number: N) -> bool;
		/// Fetch DKG public key for queued authorities
		fn next_dkg_pub_key() -> Option<Vec<u8>>;
		/// Fetch DKG public key for current authorities
		fn dkg_pub_key() -> Option<Vec<u8>>;
		/// Get list of unsigned proposals
		fn get_unsigned_proposals() -> Vec<((ChainIdType<ChainId>, DKGPayloadKey), Proposal)>;
		/// Get maximum delay before which an offchain extrinsic should be submitted
		fn get_max_extrinsic_delay(_block_number: N) -> N;
		/// Current and Queued Authority Account Ids [/current_authorities/, /next_authorities/]
		fn get_authority_accounts() -> (Vec<AccountId>, Vec<AccountId>);
		/// Reputations for authorities
		fn get_reputations(authorities: Vec<AuthorityId>) -> Vec<(AuthorityId, u32)>;
		/// Fetch DKG public key for sig
		fn next_pub_key_sig() -> Option<Vec<u8>>;
		/// Get next nonce value for refresh proposal
		fn refresh_nonce() -> u32;
		/// Get the time to restart for the dkg keygen
		fn time_to_restart() -> N;
	}
}
