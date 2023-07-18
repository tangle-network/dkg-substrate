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
use frame_support::{
	pallet_prelude::{ConstU32, Get},
	BoundedVec, RuntimeDebug,
};
use sp_std::hash::{Hash, Hasher};

use codec::{Decode, Encode, EncodeLike};
use sp_std::{vec, vec::Vec};

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub use webb_proposals::{
	FunctionSignature, Nonce as ProposalNonce, Proposal, ProposalHeader, ProposalKind, ResourceId,
	TypedChainId,
};

use crate::MaxDkgPublicKeyLength;

#[derive(
	Clone, RuntimeDebug, PartialEq, Eq, PartialOrd, Ord, scale_info::TypeInfo, codec::MaxEncodedLen,
)]
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
	pub pub_key: BoundedVec<u8, MaxDkgPublicKeyLength>,
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
			pub_key: Default::default(),
		}
	}
}

impl Decode for RefreshProposal {
	fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
		let mut data = [0u8; Self::LENGTH];
		input.read(&mut data).map_err(|_| {
			codec::Error::from("input bytes are less than the expected size (112 bytes)")
		})?;
		let mut voter_merkle_root_bytes = [0u8; 32];
		let mut session_length_bytes = [0u8; 8];
		let mut voter_count_bytes = [0u8; 4];
		let mut nonce_bytes = [0u8; ProposalNonce::LENGTH];
		let mut pub_key_bytes = [0u8; 64];
		voter_merkle_root_bytes.copy_from_slice(&data[0..32]);
		session_length_bytes.copy_from_slice(&data[32..40]);
		voter_count_bytes.copy_from_slice(&data[40..44]);
		nonce_bytes.copy_from_slice(&data[44..(44 + ProposalNonce::LENGTH)]);
		pub_key_bytes.copy_from_slice(&data[(44 + ProposalNonce::LENGTH)..]);
		let voter_merkle_root = voter_merkle_root_bytes;
		let session_length = u64::from_be_bytes(session_length_bytes);
		let voter_count = u32::from_be_bytes(voter_count_bytes);
		let nonce = ProposalNonce::from(nonce_bytes);
		let pub_key = pub_key_bytes.to_vec().try_into().map_err(|_| {
			codec::Error::from("can not fit public key bytes into 64 bytes")
		})?;

		Ok(Self { voter_merkle_root, session_length, voter_count, nonce, pub_key })
	}
}

impl Encode for RefreshProposal {
	fn encode(&self) -> Vec<u8> {
		let mut ret = vec![0u8; Self::LENGTH];
		let voter_merkle_root = self.voter_merkle_root;
		let session_length = self.session_length.to_be_bytes();
		let voter_count = self.voter_count.to_be_bytes();
		let nonce = self.nonce.to_bytes();
		let pub_key = self.pub_key.as_slice();
		ret[0..32].copy_from_slice(&voter_merkle_root);
		ret[32..40].copy_from_slice(&session_length);
		ret[40..44].copy_from_slice(&voter_count);
		ret[44..(44 + ProposalNonce::LENGTH)].copy_from_slice(&nonce);
		ret[(44 + ProposalNonce::LENGTH)..].copy_from_slice(pub_key);
		ret
	}

	fn encoded_size(&self) -> usize {
		Self::LENGTH
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
pub trait ProposalHandlerTrait {
	type BatchId;
	type MaxProposalLength: Get<u32>;
	type MaxProposals: Get<u32>;
	type MaxSignatureLen: Get<u32>;

	fn handle_unsigned_proposal(
		_prop: Proposal<Self::MaxProposalLength>,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_signed_proposal_batch(
		_prop: SignedProposalBatch<
			Self::BatchId,
			Self::MaxProposalLength,
			Self::MaxProposals,
			Self::MaxSignatureLen,
		>,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}
}

impl ProposalHandlerTrait for () {
	type BatchId = u32;
	type MaxProposalLength = crate::MaxProposalLength;
	type MaxProposals = ConstU32<0>;
	type MaxSignatureLen = ConstU32<0>;
}

/// An unsigned proposal represented in pallet storage
/// We store the creation timestamp to purge expired proposals
#[derive(
	Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo, codec::MaxEncodedLen,
)]
pub struct StoredUnsignedProposalBatch<
	BatchId,
	MaxLength: Get<u32> + Clone,
	MaxProposals: Get<u32>,
	Timestamp,
> {
	/// the batch id for this batch of proposals
	pub batch_id: BatchId,
	/// Proposals data
	pub proposals: BoundedVec<crate::UnsignedProposal<MaxLength>, MaxProposals>,
	/// Creation timestamp
	pub timestamp: Timestamp,
}

impl<BatchId, MaxLength: Get<u32> + Clone, MaxProposals: Get<u32>, Timestamp>
	StoredUnsignedProposalBatch<BatchId, MaxLength, MaxProposals, Timestamp>
{
	// We generate the data to sign for a proposal batch by doing ethabi::encode
	// on the data of all proposals in the batch
	pub fn data(&self) -> Vec<u8> {
		use ethabi::token::Token;

		// if the proposal batch has just one proposal, the data is the encoded data of
		// the proposal, this allows us to quickly verify batches with just one proposal
		if self.proposals.len() == 1 {
			return self.proposals.first().expect("checked above that len = 1").data().clone()
		}

		let mut vec_proposal_data: Vec<Token> = Vec::new();
		for proposal in self.proposals.iter() {
			let data_as_token = Token::FixedBytes(proposal.data().to_vec());
			vec_proposal_data.push(data_as_token);
		}

		ethabi::encode(&[Token::Array(vec_proposal_data)])
	}

	pub fn hash(&self) -> Option<[u8; 32]> {
		Some(crate::keccak_256(&self.data()))
	}
}

/// An unsigned proposal represented in pallet storage
/// We store the creation timestamp to purge expired proposals
#[derive(
	Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo, codec::MaxEncodedLen,
)]
pub struct SignedProposalBatch<
	BatchId,
	MaxLength: Get<u32>,
	MaxProposals: Get<u32>,
	MaxSignatureLen: Get<u32>,
> {
	/// the batch id for this batch of proposals
	pub batch_id: BatchId,
	/// Proposals data
	pub proposals: BoundedVec<Proposal<MaxLength>, MaxProposals>,
	/// Signature for proposals
	pub signature: BoundedVec<u8, MaxSignatureLen>,
}

impl<BatchId, MaxLength: Get<u32>, MaxProposals: Get<u32>, MaxSignatureLen: Get<u32>>
	From<DKGSignedPayload<BatchId, MaxLength, MaxProposals, MaxSignatureLen>>
	for SignedProposalBatch<BatchId, MaxLength, MaxProposals, MaxSignatureLen>
{
	fn from(payload: DKGSignedPayload<BatchId, MaxLength, MaxProposals, MaxSignatureLen>) -> Self {
		SignedProposalBatch {
			batch_id: payload.batch_id,
			proposals: payload.payload,
			signature: payload.signature,
		}
	}
}

impl<BatchId, MaxLength: Get<u32>, MaxProposals: Get<u32>, MaxSignatureLen: Get<u32>>
	SignedProposalBatch<BatchId, MaxLength, MaxProposals, MaxSignatureLen>
{
	// We generate the data to sign for a proposal batch by doing ethabi::encode
	// on the data of all proposals in the batch
	pub fn data(&self) -> Vec<u8> {
		use ethabi::token::Token;

		// if the proposal batch has just one proposal, the data is the encoded data of
		// the proposal, this allows us to quickly verify batches with just one proposal
		if self.proposals.len() == 1 {
			return self.proposals.first().expect("checked above that len = 1").data().clone()
		}

		let mut vec_proposal_data: Vec<Token> = Vec::new();
		for proposal in self.proposals.iter() {
			let data_as_token = Token::FixedBytes(proposal.data().to_vec());
			vec_proposal_data.push(data_as_token);
		}

		ethabi::encode(&[Token::Array(vec_proposal_data)])
	}
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGSignedPayload<
	BatchId,
	MaxLength: Get<u32>,
	MaxProposals: Get<u32>,
	MaxSignatureLen: Get<u32>,
> {
	/// Payload key
	pub batch_id: BatchId,
	/// The payload signatures are collected for.
	pub payload: BoundedVec<Proposal<MaxLength>, MaxProposals>,
	/// Runtime compatible signature for the payload
	pub signature: BoundedVec<u8, MaxSignatureLen>,
}

#[derive(Clone, Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct OffchainSignedProposalBatches<
	BatchId,
	MaxLength: Get<u32>,
	MaxProposals: Get<u32>,
	MaxSignatureLen: Get<u32>,
> {
	pub batches: Vec<SignedProposalBatch<BatchId, MaxLength, MaxProposals, MaxSignatureLen>>,
}

impl<BatchId, MaxLength: Get<u32>, MaxProposals: Get<u32>, MaxSignatureLen: Get<u32>> Default
	for OffchainSignedProposalBatches<BatchId, MaxLength, MaxProposals, MaxSignatureLen>
{
	fn default() -> Self {
		Self { batches: Default::default() }
	}
}
