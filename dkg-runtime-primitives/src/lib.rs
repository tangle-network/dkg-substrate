// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
#![cfg_attr(not(feature = "std"), no_std)]
// NOTE: needed to silence warnings about generated code in `decl_runtime_apis`
#![allow(clippy::too_many_arguments, clippy::unnecessary_mut_passed)]

pub mod handlers;
pub mod offchain;
pub mod proposal;
pub mod traits;
pub mod utils;
pub use crate::proposal::DKGPayloadKey;
use codec::{Codec, Decode, Encode, MaxEncodedLen};
use crypto::AuthorityId;
pub use ethereum::*;
pub use ethereum_types::*;
use frame_support::{pallet_prelude::Get, BoundedVec, RuntimeDebug};
pub use proposal::*;
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{
	traits::{IdentifyAccount, Verify},
	MultiSignature,
};
use sp_std::{fmt::Debug, prelude::*, vec::Vec};
use tiny_keccak::{Hasher, Keccak};
use webb_proposals::Proposal;

/// A custom get Struct to allow to import runtime values into the dkg gadget
#[derive(
	Clone,
	Encode,
	Decode,
	Debug,
	Eq,
	PartialEq,
	scale_info::TypeInfo,
	Ord,
	PartialOrd,
	MaxEncodedLen,
	Default,
)]
pub struct CustomU32Getter<const T: u32>;

impl<const T: u32> Get<u32> for CustomU32Getter<T> {
	fn get() -> u32 {
		T
	}
}

/// Utility fn to calculate keccak 256 has
pub fn keccak_256(data: &[u8]) -> [u8; 32] {
	let mut keccak = Keccak::v256();
	keccak.update(data);
	let mut output = [0u8; 32];
	keccak.finalize(&mut output);
	output
}

/// A typedef for keygen set / session id
pub type SessionId = u64;
/// The type used to represent an MMR root hash.
pub type MmrRootHash = H256;

/// Authority set id starts with zero at genesis
pub const GENESIS_AUTHORITY_SET_ID: u64 = 0;

/// The keygen timeout limit in blocks before we consider misbehaviours
pub const KEYGEN_TIMEOUT: u32 = 10;

/// The sign timeout limit in blocks before we consider proposal as stalled
pub const SIGN_TIMEOUT: u32 = 10;

/// So long as the associated block id is within this tolerance, we consider the message as
/// deliverable. This should be less than the SIGN_TIMEOUT
pub const ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE: u64 = (SIGN_TIMEOUT - 1) as u64;

pub const fn associated_block_id_acceptable(expected: u64, received: u64) -> bool {
	// favor explicit logic for readability
	let is_acceptable_above = received >= expected &&
		received <= expected.saturating_add(ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE);
	let is_acceptable_below = received < expected &&
		received >= expected.saturating_sub(ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE);
	let is_equal = expected == received;

	is_acceptable_above || is_acceptable_below || is_equal
}

// Engine ID for DKG
pub const DKG_ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

// Key type for DKG keys
pub const KEY_TYPE: sp_application_crypto::KeyTypeId = sp_application_crypto::KeyTypeId(*b"wdkg");

// Max length for proposals
pub type MaxProposalLength = CustomU32Getter<10_000>;

// Max authorities
pub type MaxAuthorities = CustomU32Getter<100>;

// Max reporters
pub type MaxReporters = CustomU32Getter<100>;

/// Max size for signatures
pub type MaxSignatureLength = CustomU32Getter<512>;

/// Max size for signatures
pub type MaxKeyLength = CustomU32Getter<512>;

/// Max votes to store onchain
pub type MaxVotes = CustomU32Getter<100>;

/// Max resources to store onchain
pub type MaxResources = CustomU32Getter<32>;

// Untrack interval for unsigned proposals completed stages for signing
pub const UNTRACK_INTERVAL: u32 = 10;

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct OffchainSignedProposals<BlockNumber, MaxLength: Get<u32>> {
	pub proposals: Vec<(Vec<Proposal<MaxLength>>, BlockNumber)>,
}

pub type PublicKeyAndSignature = (Vec<u8>, Vec<u8>);

#[derive(Eq, PartialEq, Clone, Encode, Default, Decode, RuntimeDebug, TypeInfo)]
pub struct AggregatedPublicKeys {
	/// A vector of public keys and signature pairs [/public_key/] , [/signature/]
	pub keys_and_signatures: Vec<PublicKeyAndSignature>,
}

#[derive(Debug, Clone, Copy, Decode, Encode, PartialEq, Eq, TypeInfo, Hash, MaxEncodedLen)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum MisbehaviourType {
	Keygen,
	Sign,
}

#[derive(Eq, PartialEq, Clone, Encode, Decode, Debug, TypeInfo, codec::MaxEncodedLen)]
pub struct AggregatedMisbehaviourReports<
	DKGId: AsRef<[u8]>,
	MaxSignatureLength: Get<u32> + Debug + Clone + TypeInfo,
	MaxReporters: Get<u32> + Debug + Clone + TypeInfo,
> {
	/// Offending type
	pub misbehaviour_type: MisbehaviourType,
	/// The round id the offense took place in
	pub session_id: u64,
	/// The offending authority
	pub offender: DKGId,
	/// A list of reporters
	pub reporters: BoundedVec<DKGId, MaxReporters>,
	/// A list of signed reports
	pub signatures: BoundedVec<BoundedVec<u8, MaxSignatureLength>, MaxReporters>,
}

impl<BlockNumber, MaxLength: Get<u32>> Default for OffchainSignedProposals<BlockNumber, MaxLength> {
	fn default() -> Self {
		Self { proposals: Default::default() }
	}
}

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
pub struct AuthoritySet<AuthorityId, MaxAuthorities: Get<u32>> {
	/// Public keys of the validator set elements
	pub authorities: BoundedVec<AuthorityId, MaxAuthorities>,
	/// Identifier of the validator set
	pub id: AuthoritySetId,
}

impl<MaxAuthorities: Get<u32>> Default for AuthoritySet<AuthorityId, MaxAuthorities> {
	fn default() -> Self {
		Self { authorities: Default::default(), id: Default::default() }
	}
}

impl<AuthorityId, MaxAuthorities: Get<u32>> AuthoritySet<AuthorityId, MaxAuthorities> {
	/// Return an empty validator set with id of 0.
	pub fn empty() -> Self {
		Self { authorities: Default::default(), id: Default::default() }
	}
}

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode, TypeInfo)]
pub enum DKGReport {
	KeygenMisbehaviour { session_id: SessionId, offender: AuthorityId },
	SignMisbehaviour { session_id: SessionId, offender: AuthorityId },
}

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct Commitment<TBlockNumber, TPayload> {
	pub payload: TPayload,
	pub block_number: TBlockNumber,
	pub validator_set_id: AuthoritySetId,
}

pub type AuthorityIndex = u32;

#[derive(Decode, Encode)]
pub enum ConsensusLog<AuthorityId: Codec, MaxAuthorities: Get<u32>> {
	/// The authorities have changed.
	#[codec(index = 1)]
	AuthoritiesChange {
		active: AuthoritySet<AuthorityId, MaxAuthorities>,
		queued: AuthoritySet<AuthorityId, MaxAuthorities>,
	},
	/// Disable the authority with given index.
	#[codec(index = 2)]
	OnDisabled(AuthorityIndex),
	/// The DKG keys have changed
	#[codec(index = 4)]
	KeyRefresh {
		forced: bool,
		old_public_key: Vec<u8>,
		new_public_key: Vec<u8>,
		new_key_signature: Vec<u8>,
	},
}

type AccountId = <<MultiSignature as Verify>::Signer as IdentifyAccount>::AccountId;

#[derive(Eq, PartialEq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct UnsignedProposal<MaxProposalLength: Get<u32> + Clone> {
	pub typed_chain_id: webb_proposals::TypedChainId,
	pub key: DKGPayloadKey,
	pub proposal: Proposal<MaxProposalLength>,
}

impl<MaxProposalLength: Get<u32> + Clone> UnsignedProposal<MaxProposalLength> {
	#[cfg(feature = "testing")]
	#[allow(clippy::unwrap_used)] // allow unwraps in tests
	pub fn testing_dummy(data: Vec<u8>) -> Self {
		let data = BoundedVec::try_from(data).unwrap();
		Self {
			typed_chain_id: webb_proposals::TypedChainId::None,
			key: DKGPayloadKey::AnchorCreateProposal(webb_proposals::Nonce(0)),
			proposal: Proposal::Unsigned { kind: ProposalKind::AnchorCreate, data },
		}
	}
	pub fn hash(&self) -> Option<[u8; 32]> {
		if let Proposal::Unsigned { data, .. } = &self.proposal {
			Some(keccak_256(data))
		} else {
			None
		}
	}

	pub fn data(&self) -> &Vec<u8> {
		match &self.proposal {
			Proposal::Unsigned { data, .. } | Proposal::Signed { data, .. } => data,
		}
	}
}

sp_api::decl_runtime_apis! {

	pub trait DKGApi<AuthorityId, N, MaxProposalLength, MaxAuthorities> where
		AuthorityId: Codec + PartialEq,
		MaxProposalLength: Get<u32> + Clone,
		MaxAuthorities : Get<u32> + Clone,
		N: Codec + PartialEq + sp_runtime::traits::AtLeast32BitUnsigned,
	{
		/// Return the current active authority set
		fn authority_set() -> AuthoritySet<AuthorityId, MaxAuthorities>;
		/// Return the current best authority set chosen for keygen
		fn get_best_authorities() -> Vec<(u16, AuthorityId)>;
		/// Return the next best authority set chosen for the queued keygen
		fn get_next_best_authorities() -> Vec<(u16, AuthorityId)>;
		/// Returns the progress of current session
		fn get_current_session_progress(block_number : N) -> Option<sp_runtime::Permill>;
		/// Return the current signature threshold for the DKG
		fn signature_threshold() -> u16;
		/// Return the current keygen threshold for the DKG
		fn keygen_threshold() -> u16;
		/// Return the next signature threshold for the DKG
		fn next_signature_threshold() -> u16;
		/// Return the next keygen threshold for the DKG
		fn next_keygen_threshold() -> u16;
		/// Return the next authorities active authority set
		fn queued_authority_set() -> AuthoritySet<AuthorityId, MaxAuthorities>;
		/// Check if refresh process should start
		fn should_refresh(_block_number: N) -> bool;
		/// Fetch DKG public key for queued authorities
		fn next_dkg_pub_key() -> Option<(AuthoritySetId, Vec<u8>)>;
		/// Fetch DKG public key for current authorities
		fn dkg_pub_key() -> (AuthoritySetId, Vec<u8>);
		/// Get list of unsigned proposals
		fn get_unsigned_proposals() -> Vec<(UnsignedProposal<MaxProposalLength>, N)>;
		/// Get maximum delay before which an offchain extrinsic should be submitted
		fn get_max_extrinsic_delay(block_number: N) -> N;
		/// Current and Queued Authority Account Ids [/current_authorities/, /next_authorities/]
		fn get_authority_accounts() -> (Vec<AccountId>, Vec<AccountId>);
		/// Reputations for authorities
		fn get_reputations(authorities: Vec<AuthorityId>) -> Vec<(AuthorityId, u128)>;
		/// Returns the set of jailed keygen authorities from a set of authorities
		fn get_keygen_jailed(set: Vec<AuthorityId>) -> Vec<AuthorityId>;
		/// Returns the set of jailed signing authorities from a set of authorities
		fn get_signing_jailed(set: Vec<AuthorityId>) -> Vec<AuthorityId>;
		/// Fetch DKG public key for sig
		fn next_pub_key_sig() -> Option<Vec<u8>>;
		/// Get next nonce value for refresh proposal
		fn refresh_nonce() -> u32;
		/// Returns true if we should execute an new keygen.
		fn should_execute_new_keygen() -> bool;
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		associated_block_id_acceptable, ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE,
		SIGN_TIMEOUT,
	};

	#[test]
	fn assert_value() {
		assert!(ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE > 0);
		assert!(ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE < SIGN_TIMEOUT as _);
	}

	#[test]
	fn test_range_above() {
		let current_block: u64 = 10;
		assert!(associated_block_id_acceptable(current_block, current_block));
		assert!(associated_block_id_acceptable(current_block, current_block + 1));
		assert!(associated_block_id_acceptable(
			current_block,
			current_block + ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE
		));
		assert!(!associated_block_id_acceptable(
			current_block,
			current_block + ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE + 1
		));
	}

	#[test]
	fn test_range_below() {
		let current_block: u64 = 10;
		assert!(associated_block_id_acceptable(current_block, current_block));
		assert!(associated_block_id_acceptable(current_block, current_block - 1));
		assert!(associated_block_id_acceptable(
			current_block,
			current_block - ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE
		));
		assert!(!associated_block_id_acceptable(
			current_block,
			current_block - ASSOCIATED_BLOCK_ID_MESSAGE_DELIVERY_TOLERANCE - 1
		));
	}
}
