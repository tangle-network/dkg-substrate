#![cfg_attr(not(feature = "std"), no_std)]

sp_api::decl_runtime_apis! {
	pub trait DKGProposalHandlerApi<Proposal> {
		fn get_unsigned_proposals() -> Vec<Proposal>;
	}
}
