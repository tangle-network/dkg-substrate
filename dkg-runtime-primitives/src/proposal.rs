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
}

pub trait ProposalHandlerTrait {
	fn handle_proposal(
		proposal: Vec<u8>,
		action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult;
}
