pub type DepositNonce = u64;
pub enum ProposalAction {
	// sign the proposal with some priority
	Sign(u8),
}

pub trait ProposalHandlerTrait<Proposal> {
	fn handle_proposal(
		proposal: Proposal,
		action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult;
}

pub trait ProposalsTrait<Proposal> {
	fn proposal_exists(chain_id: u64, nonce: DepositNonce, prop: Proposal) -> bool;
	fn remove_proposal(chain_id: u64, nonce: DepositNonce, prop: Proposal) -> bool;
}
