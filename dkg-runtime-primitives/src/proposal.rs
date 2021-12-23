use sp_std::hash::{Hash, Hasher};

use codec::{Decode, Encode};
use sp_std::vec::Vec;

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub type ProposalNonce = u64;

#[derive(
	Debug, Clone, Decode, Encode, Copy, PartialEq, Eq, PartialOrd, Ord, scale_info::TypeInfo,
)]
pub enum DKGPayloadKey {
	EVMProposal(ProposalNonce), // TODO: new voting types here
	RefreshVote(u64),
	AnchorUpdateProposal(ProposalNonce),
	TokenUpdateProposal(ProposalNonce),
	WrappingFeeUpdateProposal(ProposalNonce),
	ResourceIdUpdateProposal(ProposalNonce),
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
pub enum ProposalType {
	EVMUnsigned { data: Vec<u8> },
	EVMSigned { data: Vec<u8>, signature: Vec<u8> },
	AnchorUpdate { data: Vec<u8> },
	AnchorUpdateSigned { data: Vec<u8>, signature: Vec<u8> },
	TokenUpdate { data: Vec<u8> },
	TokenUpdateSigned { data: Vec<u8>, signature: Vec<u8> },
	WrappingFeeUpdate { data: Vec<u8> },
	WrappingFeeUpdateSigned { data: Vec<u8>, signature: Vec<u8> },
	ResourceIdUpdate { data: Vec<u8> },
	ResourceIdUpdateSigned { data: Vec<u8>, signature: Vec<u8> },
}

impl ProposalType {
	pub fn data(&self) -> Vec<u8> {
		match self {
			ProposalType::EVMUnsigned { data } => data.clone(),
			ProposalType::EVMSigned { data, .. } => data.clone(),
			ProposalType::AnchorUpdate { data } => data.clone(),
			ProposalType::AnchorUpdateSigned { data, .. } => data.clone(),
			ProposalType::TokenUpdate { data } => data.clone(),
			ProposalType::TokenUpdateSigned { data, .. } => data.clone(),
			ProposalType::WrappingFeeUpdate { data } => data.clone(),
			ProposalType::WrappingFeeUpdateSigned { data, .. } => data.clone(),
			ProposalType::ResourceIdUpdate { data } => data.clone(),
			ProposalType::ResourceIdUpdateSigned { data, .. } => data.clone(),
		}
	}

	pub fn signature(&self) -> Vec<u8> {
		match self {
			ProposalType::EVMUnsigned { .. } => Vec::new(),
			ProposalType::EVMSigned { signature, .. } => signature.clone(),
			ProposalType::AnchorUpdate { .. } => Vec::new(),
			ProposalType::AnchorUpdateSigned { signature, .. } => signature.clone(),
			ProposalType::TokenUpdate { .. } => Vec::new(),
			ProposalType::TokenUpdateSigned { signature, .. } => signature.clone(),
			ProposalType::WrappingFeeUpdate { .. } => Vec::new(),
			ProposalType::WrappingFeeUpdateSigned { signature, .. } => signature.clone(),
			ProposalType::ResourceIdUpdate { .. } => Vec::new(),
			ProposalType::ResourceIdUpdateSigned { signature, .. } => signature.clone(),
		}
	}
}

pub trait ProposalHandlerTrait {
	fn handle_unsigned_proposal(
		proposal: Vec<u8>,
		action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_signed_proposal(
		prop: ProposalType,
		payload_key: DKGPayloadKey,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_evm_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_anchor_update_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_token_update_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_wrapping_fee_update_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult;

	fn handle_resource_id_update_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult;
}
