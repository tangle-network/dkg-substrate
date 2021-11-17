use codec::{Decode, Encode};
use sp_std::vec::Vec;

pub type DepositNonce = u64;
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
		proposal: ProposalType,
		action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult;
}

pub trait ProposalsTrait {
	fn proposal_exists(chain_id: u64, nonce: DepositNonce, prop: ProposalType) -> bool;
	fn remove_proposal(chain_id: u64, nonce: DepositNonce, prop: ProposalType) -> bool;
}
