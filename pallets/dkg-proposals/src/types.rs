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
use codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;
use sp_std::{prelude::*, vec};
use frame_support::BoundedVec;
use frame_support::pallet_prelude::Get;

pub const DKG_DEFAULT_PROPOSER_THRESHOLD: u32 = 1;

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum ProposalStatus {
	Initiated,
	Approved,
	Rejected,
}

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct ProposalVotes<AccountId, BlockNumber, MaxVotes : Get<u32>> {
	pub votes_for: BoundedVec<AccountId, MaxVotes>,
	pub votes_against: BoundedVec<AccountId, MaxVotes>,
	pub status: ProposalStatus,
	pub expiry: BlockNumber,
}

impl<A: PartialEq, B: PartialOrd + Default, MaxVotes : Get<u32>> ProposalVotes<A, B, MaxVotes> {
	/// Attempts to mark the proposal as approve or rejected.
	/// Returns true if the status changes from active.
	pub fn try_to_complete(&mut self, threshold: u32, total: u32) -> ProposalStatus {
		if self.votes_for.len() >= threshold as usize {
			self.status = ProposalStatus::Approved;
			ProposalStatus::Approved
		} else if total >= threshold && self.votes_against.len() as u32 + threshold > total {
			self.status = ProposalStatus::Rejected;
			ProposalStatus::Rejected
		} else {
			ProposalStatus::Initiated
		}
	}

	/// Returns true if the proposal has been rejected or approved, otherwise
	/// false.
	pub fn is_complete(&self) -> bool {
		self.status != ProposalStatus::Initiated
	}

	/// Returns true if `who` has voted for or against the proposal
	pub fn has_voted(&self, who: &A) -> bool {
		self.votes_for.contains(who) || self.votes_against.contains(who)
	}

	/// Return true if the expiry time has been reached
	pub fn is_expired(&self, now: B) -> bool {
		self.expiry <= now
	}
}

impl<AccountId, BlockNumber: Default, MaxVotes : Get<u32>> Default for ProposalVotes<AccountId, BlockNumber, MaxVotes> {
	fn default() -> Self {
		Self {
			votes_for: Default::default(),
			votes_against: Default:default(),
			status: ProposalStatus::Initiated,
			expiry: BlockNumber::default(),
		}
	}
}
