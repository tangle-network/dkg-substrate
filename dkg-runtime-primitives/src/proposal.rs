use codec::{Decode, Encode};
use sp_std::vec::Vec;

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub type ProposalNonce = u64;
pub enum ProposalAction {
	// sign the proposal with some priority
	Sign(u8),
}

#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub enum ProposalType {
	EVMUnsigned { data: Vec<u8> },
	EVMSigned { data: Vec<u8>, signature: Vec<u8> },
}

pub trait ProposalHandlerTrait {
	fn handle_proposal(
		proposal: Vec<u8>,
		action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult;
}
