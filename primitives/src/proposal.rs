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
