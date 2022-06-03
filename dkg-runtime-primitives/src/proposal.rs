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
use frame_support::RuntimeDebug;
use sp_std::hash::{Hash, Hasher};

use codec::{Decode, Encode};
use sp_std::vec::Vec;

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub use webb_proposals::{
	FunctionSignature, Nonce as ProposalNonce, ProposalHeader, ResourceId, TypedChainId,
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

#[derive(Debug, Clone, Decode, Encode, Copy, Eq, PartialOrd, Ord, scale_info::TypeInfo)]
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

#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub enum Proposal {
	Signed { kind: ProposalKind, data: Vec<u8>, signature: Vec<u8> },
	Unsigned { kind: ProposalKind, data: Vec<u8> },
}

#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub enum ProposalKind {
	Refresh,
	ProposerSetUpdate,
	EVM,
	AnchorCreate,
	AnchorUpdate,
	TokenAdd,
	TokenRemove,
	WrappingFeeUpdate,
	ResourceIdUpdate,
	RescueTokens,
	MaxDepositLimitUpdate,
	MinWithdrawalLimitUpdate,
	SetVerifier,
	SetTreasuryHandler,
	FeeRecipientUpdate,
}

impl Proposal {
	pub fn data(&self) -> &Vec<u8> {
		match self {
			Proposal::Signed { data, .. } | Proposal::Unsigned { data, .. } => data,
		}
	}

	pub fn signature(&self) -> Option<Vec<u8>> {
		match self {
			Proposal::Signed { signature, .. } => Some(signature.clone()),
			Proposal::Unsigned { .. } => None,
		}
	}

	pub fn kind(&self) -> ProposalKind {
		match self {
			Proposal::Signed { kind, .. } | Proposal::Unsigned { kind, .. } => kind.clone(),
		}
	}

	pub fn is_signed(&self) -> bool {
		matches!(self, Proposal::Signed { .. })
	}

	pub fn is_unsigned(&self) -> bool {
		matches!(self, Proposal::Unsigned { .. })
	}

	pub fn get_payload_key(&self, nonce: ProposalNonce) -> DKGPayloadKey {
		match self.kind() {
			ProposalKind::EVM => DKGPayloadKey::EVMProposal(nonce),
			ProposalKind::AnchorCreate => DKGPayloadKey::AnchorCreateProposal(nonce),
			ProposalKind::AnchorUpdate => DKGPayloadKey::AnchorUpdateProposal(nonce),
			ProposalKind::TokenAdd => DKGPayloadKey::TokenAddProposal(nonce),
			ProposalKind::TokenRemove => DKGPayloadKey::TokenRemoveProposal(nonce),
			ProposalKind::WrappingFeeUpdate => DKGPayloadKey::WrappingFeeUpdateProposal(nonce),
			ProposalKind::ResourceIdUpdate => DKGPayloadKey::ResourceIdUpdateProposal(nonce),
			ProposalKind::RescueTokens => DKGPayloadKey::RescueTokensProposal(nonce),
			ProposalKind::MaxDepositLimitUpdate =>
				DKGPayloadKey::MaxDepositLimitUpdateProposal(nonce),
			ProposalKind::MinWithdrawalLimitUpdate =>
				DKGPayloadKey::MinWithdrawalLimitUpdateProposal(nonce),
			ProposalKind::SetVerifier => DKGPayloadKey::SetVerifierProposal(nonce),
			ProposalKind::SetTreasuryHandler => DKGPayloadKey::SetTreasuryHandlerProposal(nonce),
			ProposalKind::FeeRecipientUpdate => DKGPayloadKey::FeeRecipientUpdateProposal(nonce),
			ProposalKind::Refresh => DKGPayloadKey::RefreshVote(nonce),
			ProposalKind::ProposerSetUpdate => DKGPayloadKey::ProposerSetUpdateProposal(nonce),
		}
	}
}

pub trait ProposalHandlerTrait {
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

	fn handle_signed_proposal(_prop: Proposal) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_unsigned_refresh_proposal(
		_proposal: RefreshProposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}

	fn handle_signed_refresh_proposal(
		_proposal: RefreshProposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(())
	}
}

impl ProposalHandlerTrait for () {}

/// An unsigned proposal represented in pallet storage
/// We store the creation timestamp to purge expired proposals
#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub struct StoredUnsignedProposal<Timestamp> {
	/// Proposal data
	pub proposal: Proposal,
	/// Creation timestamp
	pub timestamp : Timestamp
}
