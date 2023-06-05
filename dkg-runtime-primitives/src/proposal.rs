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

use codec::{Decode, Encode};
use sp_std::vec::Vec;

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub use webb_proposals::{
	FunctionSignature, Nonce as ProposalNonce, Proposal, ProposalHeader, ProposalKind, ResourceId,
	TypedChainId,
};

#[derive(Clone, RuntimeDebug, scale_info::TypeInfo)]
pub struct RefreshProposal {
	pub nonce: ProposalNonce,
	pub pub_key: Vec<u8>,
}

impl Decode for RefreshProposal {
	fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		let mut data = [0u8; NONCE_LEN + 64];
		input.read(&mut data).map_err(|_| {
			codec::Error::from("input bytes are less than the expected size (68 bytes)")
		})?;
		// _NOTE_: rustc won't generate bounds check for the following slice
		// since we know the length of the slice is at least 68 bytes already.
		let mut nonce_bytes = [0u8; NONCE_LEN];
		let mut pub_key_bytes = [0u8; 64];
		nonce_bytes.copy_from_slice(&data[0..NONCE_LEN]);
		pub_key_bytes.copy_from_slice(&data[NONCE_LEN..]);
		let nonce = ProposalNonce::from(nonce_bytes);
		let pub_key = pub_key_bytes.to_vec();
		Ok(Self { nonce, pub_key })
	}
}

impl Encode for RefreshProposal {
	fn encode(&self) -> Vec<u8> {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		let mut ret = [0u8; NONCE_LEN + 64];
		let nonce = self.nonce.to_bytes();
		ret[0..NONCE_LEN].copy_from_slice(&nonce);
		ret[NONCE_LEN..(NONCE_LEN + 64)].copy_from_slice(&self.pub_key);
		ret.into()
	}

	fn encoded_size(&self) -> usize {
		const NONCE_LEN: usize = ProposalNonce::LENGTH;
		NONCE_LEN + 64
	}
}

#[derive(
	Eq, PartialEq, Clone, Encode, Decode, scale_info::TypeInfo, frame_support::RuntimeDebug,
)]
pub struct RefreshProposalSigned {
	pub nonce: ProposalNonce,
	pub signature: Vec<u8>,
}

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
	RefreshVote(ProposalNonce),
	ProposerSetUpdateProposal(ProposalNonce),
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
			(Self::RefreshVote(l0), Self::RefreshVote(r0)) => l0 == r0,
			(Self::ProposerSetUpdateProposal(l0), Self::ProposerSetUpdateProposal(r0)) => l0 == r0,
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
		_proposal: Vec<u8>,
		_action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_unsigned_proposer_set_update_proposal(
		_proposal: Vec<u8>,
		_action: ProposalAction,
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

	fn handle_unsigned_refresh_proposal(
		_proposal: RefreshProposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_signed_refresh_proposal(
		_proposal: RefreshProposal,
		_signature: Vec<u8>,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}
}

impl ProposalHandlerTrait for () {
	type BatchId = u32;
	type MaxProposalLength = ConstU32<0>;
	type MaxProposals = ConstU32<0>;
	type MaxSignatureLen = ConstU32<0>;
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

/// An unsigned proposal represented in pallet storage
/// We store the creation timestamp to purge expired proposals
#[derive(
	Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo, codec::MaxEncodedLen,
)]
pub struct StoredUnsignedProposalBatch<
	BatchId,
	MaxLength: Get<u32>,
	MaxProposals: Get<u32>,
	Timestamp,
> {
	/// the batch id for this batch of proposals
	pub batch_id: BatchId,
	/// Proposals data
	pub proposals: BoundedVec<Proposal<MaxLength>, MaxProposals>,
	/// Creation timestamp
	pub timestamp: Timestamp,
}

impl<BatchId, MaxLength: Get<u32>, MaxProposals: Get<u32>, Timestamp>
	StoredUnsignedProposalBatch<BatchId, MaxLength, MaxProposals, Timestamp>
{
	// We generate the data to sign for a proposal batch by doing ethabi::encode
	// on the data of all proposals in the batch
	pub fn data(&self) -> Vec<u8> {
		use ethabi::token::Token;
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
