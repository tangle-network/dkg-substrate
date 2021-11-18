#![cfg_attr(not(feature = "std"), no_std)]

sp_api::decl_runtime_apis! {
	pub trait DKGProposalsApi<Proposal, ProposalVotes>
	where
		Proposal: Encode + Decode,
		ProposalVotes: Encode + Decode,
	{
		fn get_pending_proposals() -> Vec<Proposal>;
		fn get_pending_proposal_votes(proposal_id: u32) -> ProposalVotes;
	}
}
