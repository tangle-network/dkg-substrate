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
