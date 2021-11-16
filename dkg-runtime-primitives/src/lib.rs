#![cfg_attr(not(feature = "std"), no_std)]
// NOTE: needed to silence warnings about generated code in `decl_runtime_apis`
#![allow(clippy::too_many_arguments, clippy::unnecessary_mut_passed)]

use codec::{Codec, Decode, Encode};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_std::{prelude::*, vec::Vec};

pub mod traits;

/// The type used to represent an MMR root hash.
pub type MmrRootHash = H256;

/// Authority set id starts with zero at genesis
pub const GENESIS_AUTHORITY_SET_ID: u64 = 0;

// Engine ID for DKG
pub const DKG_ENGINE_ID: sp_runtime::ConsensusEngineId = *b"DKG_";

// Key type for DKG keys
pub const KEY_TYPE: sp_application_crypto::KeyTypeId = sp_application_crypto::KeyTypeId(*b"webb");

pub mod crypto {
	use sp_application_crypto::{app_crypto, ecdsa};
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
	AuthoritiesChange(AuthoritySet<AuthorityId>),
	/// Disable the authority with given index.
	#[codec(index = 2)]
	OnDisabled(AuthorityIndex),
	/// MMR root hash.
	#[codec(index = 3)]
	MmrRoot(MmrRootHash),
}

sp_api::decl_runtime_apis! {

	pub trait DKGApi<AuthorityId> where
		AuthorityId: Codec + PartialEq,
	{
		/// Return the current active authority set
		fn authority_set() -> AuthoritySet<AuthorityId>;
		/// Return the current signature threshold for the DKG
		fn signature_threshold() -> u16;
	}
}
