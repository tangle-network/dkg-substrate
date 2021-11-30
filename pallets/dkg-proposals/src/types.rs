use codec::{Decode, Encode};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;
use sp_std::{prelude::*, vec};

pub const DKG_DEFAULT_PROPOSER_THRESHOLD: u32 = 1;

pub type ResourceId = [u8; 32];

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum ProposalStatus {
	Initiated,
	Approved,
	Rejected,
}

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct ProposalVotes<AccountId, BlockNumber> {
	pub votes_for: Vec<AccountId>,
	pub votes_against: Vec<AccountId>,
	pub status: ProposalStatus,
	pub expiry: BlockNumber,
}

impl<A: PartialEq, B: PartialOrd + Default> ProposalVotes<A, B> {
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
		self.votes_for.contains(&who) || self.votes_against.contains(&who)
	}

	/// Return true if the expiry time has been reached
	pub fn is_expired(&self, now: B) -> bool {
		self.expiry <= now
	}
}

impl<AccountId, BlockNumber: Default> Default for ProposalVotes<AccountId, BlockNumber> {
	fn default() -> Self {
		Self {
			votes_for: vec![],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: BlockNumber::default(),
		}
	}
}
