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
use frame_support::{pallet_prelude::Get, RuntimeDebug};
use sp_std::hash::{Hash, Hasher};

use codec::{Decode, Encode, EncodeLike, MaxEncodedLen};
use sp_std::{vec, vec::Vec};

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub use webb_proposals::{
	FunctionSignature, Nonce as ProposalNonce, Proposal, ProposalHeader, ProposalKind, ResourceId,
	TypedChainId,
};

#[derive(Clone, RuntimeDebug, scale_info::TypeInfo)]
pub struct RefreshProposal {
	/// The merkle root of the voters (validators)
	pub voter_merkle_root: [u8; 32],
	/// The session length in milliseconds
	pub session_length: u64,
	/// The number of voters
	pub voter_count: u32,
	/// The refresh nonce for the rotation
	pub nonce: ProposalNonce,
	/// The public key of the governor
	pub pub_key: Vec<u8>,
}

impl RefreshProposal {
	/// Length of the proposal in bytes.
	pub const LENGTH: usize = 32 + 8 + 4 + ProposalNonce::LENGTH + 64;

	/// Build a `RefreshProposal` from raw bytes
	pub fn from(bytes: &[u8]) -> Result<Self, &'static str> {
		Decode::decode(&mut &bytes[..]).map_err(|_| "Failed to decode RefreshProposal")
	}
}

impl Default for RefreshProposal {
	fn default() -> Self {
		Self {
			voter_merkle_root: [0u8; 32],
			session_length: 0,
			voter_count: 0,
			nonce: ProposalNonce(0u32),
			pub_key: vec![],
		}
	}
}

impl MaxEncodedLen for RefreshProposal {
	fn max_encoded_len() -> usize {
		Self::LENGTH
	}
}

impl Decode for RefreshProposal {
	fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		let mut data = [0u8; Self::LENGTH];
		input.read(&mut data).map_err(|_| {
			codec::Error::from("input bytes are less than the expected size (68 bytes)")
		})?;
		// _NOTE_: rustc won't generate bounds check for the following slice
		// since we know the length of the slice is at least 68 bytes already.
		let mut voter_merkle_root_bytes = [0u8; 32];
		let mut session_length_bytes = [0u8; 8];
		let mut voter_count_bytes = [0u8; 4];
		let mut nonce_bytes = [0u8; NONCE_LEN];
		let mut pub_key_bytes = [0u8; 64];
		voter_merkle_root_bytes.copy_from_slice(&data[0..32]);
		session_length_bytes.copy_from_slice(&data[32..40]);
		voter_count_bytes.copy_from_slice(&data[40..44]);
		nonce_bytes.copy_from_slice(&data[44..(44 + NONCE_LEN)]);
		pub_key_bytes.copy_from_slice(&data[(44 + NONCE_LEN)..]);
		let voter_merkle_root = voter_merkle_root_bytes;
		let session_length = u64::from_be_bytes(session_length_bytes);
		let voter_count = u32::from_be_bytes(voter_count_bytes);
		let nonce = ProposalNonce::from(nonce_bytes);
		let pub_key = pub_key_bytes.to_vec();
		Ok(Self { voter_merkle_root, session_length, voter_count, nonce, pub_key })
	}
}

impl Encode for RefreshProposal {
	fn encode(&self) -> Vec<u8> {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		let mut ret = [0u8; 32 + 8 + 4 + NONCE_LEN + 64];
		let voter_merkle_root = self.voter_merkle_root;
		let session_length = self.session_length.to_be_bytes();
		let voter_count = self.voter_count.to_be_bytes();
		let nonce = self.nonce.to_bytes();
		let pub_key = self.pub_key.as_slice();
		ret[0..32].copy_from_slice(&voter_merkle_root);
		ret[32..40].copy_from_slice(&session_length);
		ret[40..44].copy_from_slice(&voter_count);
		ret[44..(44 + NONCE_LEN)].copy_from_slice(&nonce);
		ret[(44 + NONCE_LEN)..].copy_from_slice(pub_key);
		ret.into()
	}

	fn encoded_size(&self) -> usize {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		32 + 8 + 4 + NONCE_LEN + 64
	}
}

impl EncodeLike for RefreshProposal {}

#[derive(
	Debug,
	Clone,
	Decode,
	Encode,
	Copy,
	Eq,
	PartialOrd,
	Ord,
	scale_info::TypeInfo,
	codec::MaxEncodedLen,
)]
pub enum DKGPayloadKey {
	EVMProposal(ProposalNonce),
	RefreshProposal(ProposalNonce),
	AnchorCreateProposal(ProposalNonce),
	AnchorUpdateProposal(ProposalNonce),
	TokenAddProposal(ProposalNonce),
	TokenRemoveProposal(ProposalNonce),
	WrappingFeeUpdateProposal(ProposalNonce),
	ResourceIdUpdateProposal(ProposalNonce),
	RescueTokensProposal(ProposalNonce),
	MaxDepositLimitUpdateProposal(ProposalNonce),
	MinWithdrawalLimitUpdateProposal(ProposalNonce),
	SetVerifierProposal(ProposalNonce),
	SetTreasuryHandlerProposal(ProposalNonce),
	FeeRecipientUpdateProposal(ProposalNonce),
}

impl PartialEq for DKGPayloadKey {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::EVMProposal(l0), Self::EVMProposal(r0)) => l0 == r0,
			(Self::RefreshProposal(l0), Self::RefreshProposal(r0)) => l0 == r0,
			(Self::AnchorCreateProposal(l0), Self::AnchorCreateProposal(r0)) => l0 == r0,
			(Self::AnchorUpdateProposal(l0), Self::AnchorUpdateProposal(r0)) => l0 == r0,
			(Self::TokenAddProposal(l0), Self::TokenAddProposal(r0)) => l0 == r0,
			(Self::TokenRemoveProposal(l0), Self::TokenRemoveProposal(r0)) => l0 == r0,
			(Self::WrappingFeeUpdateProposal(l0), Self::WrappingFeeUpdateProposal(r0)) => l0 == r0,
			(Self::ResourceIdUpdateProposal(l0), Self::ResourceIdUpdateProposal(r0)) => l0 == r0,
			(Self::RescueTokensProposal(l0), Self::RescueTokensProposal(r0)) => l0 == r0,
			(Self::MaxDepositLimitUpdateProposal(l0), Self::MaxDepositLimitUpdateProposal(r0)) =>
				l0 == r0,
			(
				Self::MinWithdrawalLimitUpdateProposal(l0),
				Self::MinWithdrawalLimitUpdateProposal(r0),
			) => l0 == r0,
			(Self::SetVerifierProposal(l0), Self::SetVerifierProposal(r0)) => l0 == r0,
			(Self::SetTreasuryHandlerProposal(l0), Self::SetTreasuryHandlerProposal(r0)) =>
				l0 == r0,
			(Self::FeeRecipientUpdateProposal(l0), Self::FeeRecipientUpdateProposal(r0)) =>
				l0 == r0,
			_ => false,
		}
	}
}

impl Hash for DKGPayloadKey {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.encode().hash(state)
	}
}
pub enum ProposalAction {
	// sign the proposal with some priority
	Sign(u8),
}
pub trait ProposalHandlerTrait<MaxProposalLength: Get<u32>> {
	fn handle_unsigned_proposal(
		_prop: Proposal<MaxProposalLength>,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_signed_proposal(
		_prop: Proposal<MaxProposalLength>,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}
}

impl<M: Get<u32>> ProposalHandlerTrait<M> for () {}

/// An unsigned proposal represented in pallet storage
/// We store the creation timestamp to purge expired proposals
#[derive(
	Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo, codec::MaxEncodedLen,
)]
pub struct StoredUnsignedProposal<Timestamp, MaxLength: Get<u32>> {
	/// Proposal data
	pub proposal: Proposal<MaxLength>,
	/// Creation timestamp
	pub timestamp: Timestamp,
}
