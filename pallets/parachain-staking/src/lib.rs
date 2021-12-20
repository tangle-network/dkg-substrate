// Copyright 2019-2021 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

//! # Parachain Staking
//! Minimal staking pallet that implements collator selection by total backed
//! stake. The main difference between this pallet and `frame/pallet-staking` is
//! that this pallet uses direct delegation. Nominators choose exactly who they
//! nominate and with what stake. This is different from `frame/pallet-staking`
//! where you approval vote and then run Phragmen.
//!
//! ### Rules
//! There is a new round every `<Round<T>>::get().length` blocks.
//!
//! At the start of every round,
//! * issuance is distributed to collators (and their nominators) for block
//!   authoring
//! `T::RewardPaymentDelay` rounds ago
//! * queued collator and nominator exits are executed
//! * a new set of collators is chosen from the candidates
//!
//! To join the set of candidates, call `join_candidates` with `bond >=
//! MinCollatorCandidateStk`.
//!
//! To leave the set of candidates, call `leave_candidates`. If the call
//! succeeds, the collator is removed from the pool of candidates so they cannot
//! be selected for future collator sets, but they are not unstaked until
//! `T::LeaveCandidatesDelay` rounds later. The exit request is stored in the
//! `ExitQueue` and processed `T::LeaveCandidatesDelay` rounds later to unstake
//! the collator and all of its nominations.
//!
//! To join the set of nominators, call `nominate` and pass in an account that
//! is already a collator candidate and `bond >= MinNominatorStk`. Each
//! nominator can nominate up to `T::MaxCollatorsPerNominator` collator
//! candidates by calling `nominate`.
//!
//! To revoke a nomination, call `revoke_nomination` with the collator
//! candidate's account. To leave the set of nominators and revoke all
//! nominations, call `leave_nominators`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod benchmarks;
mod inflation;
#[cfg(test)]
mod mock;
mod set;
#[cfg(test)]
mod tests;

pub mod weights;

use weights::WeightInfo;

use frame_support::pallet;
pub use inflation::{InflationInfo, Range};

pub use pallet::*;

#[pallet]
pub mod pallet {
	use crate::{set::OrderedSet, InflationInfo, Range, WeightInfo};
	use frame_support::{
		dispatch::Weight,
		pallet_prelude::*,
		traits::{Currency, EstimateNextSessionRotation, Get, Imbalance, ReservableCurrency},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use pallet_session::{SessionManager, ShouldEndSession};
	use parity_scale_codec::{Decode, Encode};
	use scale_info::TypeInfo;
	use sp_runtime::{
		traits::{AccountIdConversion, AtLeast32BitUnsigned, One, Saturating, Zero},
		Perbill, Percent, Permill, RuntimeDebug,
	};
	use sp_staking::SessionIndex;
	use sp_std::{cmp::Ordering, collections::btree_map::BTreeMap, prelude::*};

	/// Pallet for parachain staking
	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
	pub struct Bond<AccountId, Balance> {
		pub owner: AccountId,
		pub amount: Balance,
	}

	impl<A, B: Default> Bond<A, B> {
		fn from_owner(owner: A) -> Self {
			Bond { owner, amount: B::default() }
		}
	}

	impl<AccountId: Ord, Balance> Eq for Bond<AccountId, Balance> {}

	impl<AccountId: Ord, Balance> Ord for Bond<AccountId, Balance> {
		fn cmp(&self, other: &Self) -> Ordering {
			self.owner.cmp(&other.owner)
		}
	}

	impl<AccountId: Ord, Balance> PartialOrd for Bond<AccountId, Balance> {
		fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
			Some(self.cmp(other))
		}
	}

	impl<AccountId: Ord, Balance> PartialEq for Bond<AccountId, Balance> {
		fn eq(&self, other: &Self) -> bool {
			self.owner == other.owner
		}
	}

	#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo)]
	/// The activity status of the collator
	pub enum CollatorStatus {
		/// Committed to be online and producing valid blocks (not equivocating)
		Active,
		/// Temporarily inactive and excused for inactivity
		Idle,
		/// Bonded until the inner round
		Leaving(RoundIndex),
	}

	impl Default for CollatorStatus {
		fn default() -> CollatorStatus {
			CollatorStatus::Active
		}
	}

	#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
	/// Snapshot of collator state at the start of the round for which they are
	/// selected
	pub struct CollatorSnapshot<AccountId, Balance> {
		pub bond: Balance,
		pub nominators: Vec<Bond<AccountId, Balance>>,
		pub total: Balance,
	}

	impl<AccountId, Balance: Default> Default for CollatorSnapshot<AccountId, Balance> {
		fn default() -> Self {
			Self { bond: Balance::default(), nominators: Vec::default(), total: Balance::default() }
		}
	}

	#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
	/// Collator state with commission fee, bonded stake, and nominations
	pub struct Collator2<AccountId, Balance> {
		/// The account of this collator
		pub id: AccountId,
		/// This collator's self stake.
		pub bond: Balance,
		/// Set of all nominator AccountIds (to prevent >1 nomination per
		/// AccountId)
		pub nominators: OrderedSet<AccountId>,
		/// Top T::MaxNominatorsPerCollator::get() nominators, ordered greatest
		/// to least
		pub top_nominators: Vec<Bond<AccountId, Balance>>,
		/// Bottom nominators (unbounded), ordered least to greatest
		pub bottom_nominators: Vec<Bond<AccountId, Balance>>,
		/// Sum of top nominations + self.bond
		pub total_counted: Balance,
		/// Sum of all nominations + self.bond = (total_counted + uncounted)
		pub total_backing: Balance,
		/// Current status of the collator
		pub state: CollatorStatus,
	}

	/// Convey relevant information describing if a nominator was added to the
	/// top or bottom Nominations added to the top yield a new total
	#[derive(Clone, Copy, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo)]
	pub enum NominatorAdded<B> {
		AddedToTop { new_total: B },
		AddedToBottom,
	}

	impl<
			A: Ord + Clone,
			B: AtLeast32BitUnsigned + Ord + Copy + sp_std::ops::AddAssign + sp_std::ops::SubAssign,
		> Collator2<A, B>
	{
		pub fn new(id: A, bond: B) -> Self {
			Collator2 {
				id,
				bond,
				nominators: OrderedSet::new(),
				top_nominators: Vec::new(),
				bottom_nominators: Vec::new(),
				total_counted: bond,
				total_backing: bond,
				state: CollatorStatus::default(), // default active
			}
		}

		pub fn is_active(&self) -> bool {
			self.state == CollatorStatus::Active
		}

		pub fn is_leaving(&self) -> bool {
			matches!(self.state, CollatorStatus::Leaving(_))
		}

		pub fn bond_more(&mut self, more: B) {
			self.bond += more;
			self.total_counted += more;
			self.total_backing += more;
		}

		// Return None if less >= self.bond => collator must leave instead of bond less
		pub fn bond_less(&mut self, less: B) -> Option<B> {
			if self.bond > less {
				self.bond -= less;
				self.total_counted -= less;
				self.total_backing -= less;
				Some(self.bond)
			} else {
				None
			}
		}

		/// Infallible sorted insertion
		/// caller must verify !self.nominators.contains(nominator.owner) before
		/// call
		pub fn add_top_nominator(&mut self, nominator: Bond<A, B>) {
			match self.top_nominators.binary_search_by(|x| nominator.amount.cmp(&x.amount)) {
				Ok(i) => self.top_nominators.insert(i, nominator),
				Err(i) => self.top_nominators.insert(i, nominator),
			}
		}

		/// Infallible sorted insertion
		/// caller must verify !self.nominators.contains(nominator.owner) before
		/// call
		pub fn add_bottom_nominator(&mut self, nominator: Bond<A, B>) {
			match self.bottom_nominators.binary_search_by(|x| x.amount.cmp(&nominator.amount)) {
				Ok(i) => self.bottom_nominators.insert(i, nominator),
				Err(i) => self.bottom_nominators.insert(i, nominator),
			}
		}

		/// Sort top nominators from greatest to least
		pub fn sort_top_nominators(&mut self) {
			self.top_nominators.sort_unstable_by(|a, b| b.amount.cmp(&a.amount));
		}

		/// Sort bottom nominators from least to greatest
		pub fn sort_bottom_nominators(&mut self) {
			self.bottom_nominators.sort_unstable_by(|a, b| a.amount.cmp(&b.amount));
		}

		/// Bond a new account as a nominator, and make a first nomination. If
		/// successful, the return value indicates whether the nomination is top
		/// for the candidate.
		pub fn add_nominator<T: Config>(
			&mut self,
			acc: A,
			amount: B,
		) -> Result<NominatorAdded<B>, DispatchError> {
			ensure!(self.nominators.insert(acc.clone()), Error::<T>::NominatorExists);
			self.total_backing += amount;
			if (self.top_nominators.len() as u32) < T::MaxNominatorsPerCollator::get() {
				self.add_top_nominator(Bond { owner: acc, amount });
				self.total_counted += amount;
				Ok(NominatorAdded::AddedToTop { new_total: self.total_counted })
			} else {
				// >pop requires push to reset in case isn't pushed to bottom
				let last_nomination_in_top = self
					.top_nominators
					.pop()
					.expect("self.top_nominators.len() >= T::Max exists >= 1 element in top");
				if amount > last_nomination_in_top.amount {
					// update total_counted with positive difference
					self.total_counted += amount - last_nomination_in_top.amount;
					// last nomination already popped from top_nominators
					// insert new nominator into top_nominators
					self.add_top_nominator(Bond { owner: acc, amount });
					self.add_bottom_nominator(last_nomination_in_top);
					Ok(NominatorAdded::AddedToTop { new_total: self.total_counted })
				} else {
					// >required push to previously popped last nomination into top_nominators
					self.top_nominators.push(last_nomination_in_top);
					self.add_bottom_nominator(Bond { owner: acc, amount });
					Ok(NominatorAdded::AddedToBottom)
				}
			}
		}

		/// Return Ok((if_total_counted_changed, nominator's stake))
		pub fn rm_nominator<T: Config>(
			&mut self,
			nominator: A,
		) -> Result<(bool, B), DispatchError> {
			ensure!(self.nominators.remove(&nominator), Error::<T>::NominatorDNEInNominatorSet);
			let mut nominator_stake: Option<B> = None;
			self.top_nominators = self
				.top_nominators
				.clone()
				.into_iter()
				.filter_map(|nom| {
					if nom.owner != nominator {
						Some(nom)
					} else {
						nominator_stake = Some(nom.amount);
						None
					}
				})
				.collect();
			// item removed from the top => highest bottom is popped from bottom and pushed
			// to top
			if let Some(s) = nominator_stake {
				// last element has largest amount as per ordering
				if let Some(last) = self.bottom_nominators.pop() {
					self.total_counted -= s - last.amount;
					self.add_top_nominator(last);
				} else {
					// no item in bottom nominators so no item from bottom to pop and push up
					self.total_counted -= s;
				}
				self.total_backing -= s;
				return Ok((true, s))
			}
			// else (no item removed from the top)
			self.bottom_nominators = self
				.bottom_nominators
				.clone()
				.into_iter()
				.filter_map(|nom| {
					if nom.owner != nominator {
						Some(nom)
					} else {
						nominator_stake = Some(nom.amount);
						None
					}
				})
				.collect();
			// if err, no item with account exists in top || bottom
			let stake = nominator_stake.ok_or(Error::<T>::NominatorDNEinTopNorBottom)?;
			self.total_backing -= stake;
			Ok((false, stake))
		}

		/// Return true if in_top after call
		/// Caller must verify before call that account is a nominator
		pub fn inc_nominator(&mut self, nominator: A, more: B) -> bool {
			let mut in_top = false;
			for x in &mut self.top_nominators {
				if x.owner == nominator {
					x.amount += more;
					self.total_counted += more;
					self.total_backing += more;
					in_top = true;
					break
				}
			}
			// if nominator was increased in top nominators
			if in_top {
				self.sort_top_nominators();
				return true
			}
			// else nominator to increase must exist in bottom
			// >pop requires push later on to reset in case it isn't used
			let lowest_top = self
				.top_nominators
				.pop()
				.expect("any bottom nominators => exists T::Max top nominators");
			let mut move_2_top = false;
			for x in &mut self.bottom_nominators {
				if x.owner == nominator {
					x.amount += more;
					self.total_backing += more;
					move_2_top = x.amount > lowest_top.amount;
					break
				}
			}
			if move_2_top {
				self.sort_bottom_nominators();
				let highest_bottom = self.bottom_nominators.pop().expect("updated => exists");
				self.total_counted += highest_bottom.amount - lowest_top.amount;
				self.add_top_nominator(highest_bottom);
				self.add_bottom_nominator(lowest_top);
				true
			} else {
				// >required push to reset top_nominators from earlier pop
				self.top_nominators.push(lowest_top);
				self.sort_bottom_nominators();
				false
			}
		}

		/// Return true if in_top after call
		pub fn dec_nominator(&mut self, nominator: A, less: B) -> bool {
			let mut in_top = false;
			let mut new_lowest_top: Option<Bond<A, B>> = None;
			for x in &mut self.top_nominators {
				if x.owner == nominator {
					x.amount -= less;
					// if there is at least 1 nominator in bottom nominators, compare it to check
					// if it should be swapped with lowest top nomination and put in top
					// >pop requires push later on to reset in case it isn't used
					if let Some(highest_bottom) = self.bottom_nominators.pop() {
						if highest_bottom.amount > x.amount {
							new_lowest_top = Some(highest_bottom);
						} else {
							// >required push to reset self.bottom_nominators
							self.bottom_nominators.push(highest_bottom);
						}
					}
					in_top = true;
					break
				}
			}
			if in_top {
				self.sort_top_nominators();
				if let Some(highest_bottom) = new_lowest_top {
					// pop last in top to swap it with top bottom
					let lowest_top = self
						.top_nominators
						.pop()
						.expect("must have >1 item to update, assign in_top = true");
					self.total_counted -= lowest_top.amount + less;
					self.total_counted += highest_bottom.amount;
					self.total_backing -= less;
					self.add_top_nominator(highest_bottom);
					self.add_bottom_nominator(lowest_top);
					return false
				} else {
					// no existing bottom nominators so update both counters the same magnitude
					self.total_counted -= less;
					self.total_backing -= less;
					return true
				}
			}
			for x in &mut self.bottom_nominators {
				if x.owner == nominator {
					x.amount -= less;
					self.total_backing -= less;
					break
				}
			}
			self.sort_bottom_nominators();
			false
		}

		pub fn go_offline(&mut self) {
			self.state = CollatorStatus::Idle;
		}

		pub fn go_online(&mut self) {
			self.state = CollatorStatus::Active;
		}

		pub fn leave(&mut self, when: RoundIndex) {
			self.state = CollatorStatus::Leaving(when);
		}
	}

	impl<A: Clone, B: Copy> From<Collator2<A, B>> for CollatorSnapshot<A, B> {
		fn from(other: Collator2<A, B>) -> CollatorSnapshot<A, B> {
			CollatorSnapshot {
				bond: other.bond,
				nominators: other.top_nominators,
				total: other.total_counted,
			}
		}
	}

	#[derive(Clone, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo)]
	pub enum NominatorStatus {
		/// Active with no scheduled exit
		Active,
		/// Schedule exit to revoke all ongoing nominations
		Leaving(RoundIndex),
	}

	#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
	/// Nominator state
	pub struct Nominator2<AccountId, Balance> {
		/// All current nominations
		pub nominations: OrderedSet<Bond<AccountId, Balance>>,
		/// Nominations scheduled to be revoked
		pub revocations: OrderedSet<AccountId>,
		/// Total balance locked for this nominator
		pub total: Balance,
		/// Total number of revocations scheduled to be executed
		pub scheduled_revocations_count: u32,
		/// Total amount to be unbonded once revocations are executed
		pub scheduled_revocations_total: Balance,
		/// Status for this nominator
		pub status: NominatorStatus,
	}

	impl<
			AccountId: Ord + Clone,
			Balance: Copy
				+ sp_std::ops::AddAssign
				+ sp_std::ops::Add<Output = Balance>
				+ sp_std::ops::SubAssign
				+ PartialOrd
				+ Zero,
		> Nominator2<AccountId, Balance>
	{
		pub fn new(collator: AccountId, amount: Balance) -> Self {
			Nominator2 {
				nominations: OrderedSet::from(vec![Bond { owner: collator, amount }]),
				revocations: OrderedSet::new(),
				total: amount,
				scheduled_revocations_count: 0u32,
				scheduled_revocations_total: Zero::zero(),
				status: NominatorStatus::Active,
			}
		}

		pub fn is_active(&self) -> bool {
			matches!(self.status, NominatorStatus::Active)
		}

		pub fn is_leaving(&self) -> bool {
			matches!(self.status, NominatorStatus::Leaving(_))
		}

		/// Set nominator status to exit
		pub fn leave(&mut self, when: RoundIndex) {
			self.status = NominatorStatus::Leaving(when)
		}

		pub fn add_nomination(&mut self, bond: Bond<AccountId, Balance>) -> bool {
			let amt = bond.amount;
			if self.nominations.insert(bond) {
				self.total += amt;
				true
			} else {
				false
			}
		}

		// Return Some(remaining balance), must be more than MinNominatorStk
		// Return None if nomination not found
		pub fn rm_nomination(&mut self, collator: AccountId) -> Option<Balance> {
			let mut amt: Option<Balance> = None;
			let nominations = self
				.nominations
				.0
				.iter()
				.filter_map(|x| {
					if x.owner == collator {
						amt = Some(x.amount);
						None
					} else {
						Some(x.clone())
					}
				})
				.collect();
			if let Some(balance) = amt {
				self.nominations = OrderedSet::from(nominations);
				self.total -= balance;
				Some(self.total)
			} else {
				None
			}
		}

		// Return false if nomination not found
		pub fn inc_nomination(&mut self, collator: AccountId, more: Balance) -> bool {
			for x in &mut self.nominations.0 {
				if x.owner == collator {
					x.amount += more;
					self.total += more;
					return true
				}
			}
			false
		}

		// Return Some(Some(balance)) if successful
		// Return None if nomination not found
		// Return Some(None) if less >= nomination_total
		pub fn dec_nomination(
			&mut self,
			collator: AccountId,
			less: Balance,
		) -> Option<Option<Balance>> {
			for x in &mut self.nominations.0 {
				if x.owner == collator {
					if x.amount > less {
						x.amount -= less;
						self.total -= less;
						return Some(Some(x.amount))
					} else {
						// must rm entire nomination if x.amount <= less
						return Some(None)
					}
				}
			}
			None
		}
	}

	/// A convertor from collators id.
	pub struct IdentityCollator;
	impl<T> sp_runtime::traits::Convert<T, Option<T>> for IdentityCollator {
		fn convert(t: T) -> Option<T> {
			Some(t)
		}
	}

	#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo)]
	/// The current round index and transition information
	pub struct RoundInfo<BlockNumber> {
		/// Current round index
		pub current: RoundIndex,
		/// The first block of the current round
		pub first: BlockNumber,
		/// The length of the current round in number of blocks
		pub length: u32,
	}
	impl<
			B: Copy
				+ sp_std::ops::Add<Output = B>
				+ sp_std::ops::Sub<Output = B>
				+ From<u32>
				+ PartialOrd,
		> RoundInfo<B>
	{
		pub fn new(current: RoundIndex, first: B, length: u32) -> RoundInfo<B> {
			RoundInfo { current, first, length }
		}

		/// Check if the round should be updated
		pub fn should_update(&self, now: B) -> bool {
			now - self.first >= self.length.into()
		}

		/// New round
		pub fn update(&mut self, now: B) {
			self.current += 1u32;
			self.first = now;
		}
	}
	impl<
			B: Copy
				+ sp_std::ops::Add<Output = B>
				+ sp_std::ops::Sub<Output = B>
				+ From<u32>
				+ PartialOrd,
		> Default for RoundInfo<B>
	{
		fn default() -> RoundInfo<B> {
			RoundInfo::new(1u32, 0u32.into(), 20u32)
		}
	}

	#[derive(Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo)]
	/// Reserve information { account, percent_of_inflation }
	pub struct ParachainBondConfig<AccountId> {
		/// Account which receives funds intended for parachain bond
		pub account: AccountId,
		/// Percent of inflation set aside for parachain bond account
		pub percent: Percent,
	}

	#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
	/// Store and process all delayed exits by collators and nominators
	pub struct ExitQ<AccountId> {
		/// Candidate exit set
		pub candidates: OrderedSet<AccountId>,
		/// Nominator exit set (does not include nominators that made `revoke`
		/// requests)
		pub nominators_leaving: OrderedSet<AccountId>,
		/// [Candidate, Round to Exit]
		pub candidate_schedule: Vec<(AccountId, RoundIndex)>,
		/// [Nominator, Some(ValidatorId) || None => All Nominations, Round To
		/// Exit]
		pub nominator_schedule: Vec<(AccountId, Option<AccountId>, RoundIndex)>,
	}

	impl<AccountId> Default for ExitQ<AccountId> {
		fn default() -> Self {
			Self {
				candidates: OrderedSet::default(),
				nominators_leaving: OrderedSet::default(),
				nominator_schedule: Vec::default(),
				candidate_schedule: Vec::default(),
			}
		}
	}

	impl<A: Ord + Clone> ExitQ<A> {
		/// Schedule to leave the set of candidates and return all ongoing
		/// nominations
		pub fn schedule_candidate_exit<T: Config>(
			&mut self,
			candidate: A,
			exit_round: RoundIndex,
		) -> DispatchResult {
			ensure!(self.candidates.insert(candidate.clone()), Error::<T>::CandidateAlreadyLeaving);
			self.candidate_schedule.push((candidate, exit_round));
			Ok(())
		}

		/// Schedule to leave the set of nominators and revoke all ongoing
		/// nominations
		pub fn schedule_nominator_exit<T: Config>(
			&mut self,
			nominator: A,
			exit_round: RoundIndex,
		) -> DispatchResult {
			ensure!(
				self.nominators_leaving.insert(nominator.clone()),
				Error::<T>::NominatorAlreadyLeaving
			);
			self.nominator_schedule.push((nominator, None, exit_round));
			Ok(())
		}

		/// Schedule to revoke a single nomination
		pub fn schedule_nomination_revocation<T: Config>(
			&mut self,
			nominator: A,
			collator: A,
			exit_round: RoundIndex,
		) -> DispatchResult {
			self.nominator_schedule.push((nominator, Some(collator), exit_round));
			Ok(())
		}
	}

	type RoundIndex = u32;
	type RewardPoint = u32;
	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	/// Configuration trait of this pallet.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Overarching event type
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// The currency type
		type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
		/// The origin for monetary governance
		type MonetaryGovernanceOrigin: EnsureOrigin<Self::Origin>;
		/// Minimum of blocks per round
		#[pallet::constant]
		type MinBlocksPerRound: Get<u32>;
		/// Number of blocks per round, this is the period used in
		/// the session pallet in rotating sessions
		#[pallet::constant]
		type BlocksPerRound: Get<u32>;
		/// Number of rounds that collators remain bonded before exit request is
		/// executed
		#[pallet::constant]
		type LeaveCandidatesDelay: Get<RoundIndex>;
		/// Number of rounds that nominators remain bonded before exit request
		/// is executed
		#[pallet::constant]
		type LeaveNominatorsDelay: Get<RoundIndex>;
		/// Number of rounds that nominations remain bonded before revocation
		/// request is executed
		#[pallet::constant]
		type RevokeNominationDelay: Get<RoundIndex>;
		/// Number of rounds after which block authors are rewarded
		#[pallet::constant]
		type RewardPaymentDelay: Get<RoundIndex>;
		/// Minimum number of selected candidates every round
		#[pallet::constant]
		type MinSelectedCandidates: Get<u32>;
		/// Maximum nominators counted per collator
		#[pallet::constant]
		type MaxNominatorsPerCollator: Get<u32>;
		/// Maximum collators per nominator
		#[pallet::constant]
		type MaxCollatorsPerNominator: Get<u32>;
		/// Default commission due to collators, set at genesis
		#[pallet::constant]
		type DefaultCollatorCommission: Get<Perbill>;
		/// Default percent of inflation set aside for parachain bond account
		#[pallet::constant]
		type DefaultParachainBondReservePercent: Get<Percent>;
		/// Minimum stake required for any account to be in `SelectedCandidates`
		/// for the round
		#[pallet::constant]
		type MinCollatorStk: Get<BalanceOf<Self>>;
		/// Minimum stake required for any account to be a collator candidate
		#[pallet::constant]
		type MinCollatorCandidateStk: Get<BalanceOf<Self>>;
		/// Minimum stake for any registered on-chain account to nominate
		#[pallet::constant]
		type MinNomination: Get<BalanceOf<Self>>;
		/// Minimum stake for any registered on-chain account to become a
		/// nominator
		#[pallet::constant]
		type MinNominatorStk: Get<BalanceOf<Self>>;
		#[pallet::constant]
		type PalletId: Get<PalletId>;
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	#[pallet::error]
	pub enum Error<T> {
		NominatorDNE,
		NominatorDNEinTopNorBottom,
		NominatorDNEInNominatorSet,
		CandidateDNE,
		NominationDNE,
		NominatorExists,
		CandidateExists,
		ValBondBelowMin,
		NomBondBelowMin,
		NominationBelowMin,
		AlreadyOffline,
		AlreadyActive,
		NominatorAlreadyLeaving,
		NominationAlreadyRevoked,
		CandidateAlreadyLeaving,
		CannotActBecauseLeaving,
		CannotActBecauseRevoking,
		ExceedMaxCollatorsPerNom,
		AlreadyNominatedCollator,
		InvalidSchedule,
		CannotSetBelowMin,
		NoWritingSameValue,
		TooLowCandidateCountWeightHintJoinCandidates,
		TooLowCollatorCandidateCountToLeaveCandidates,
		TooLowNominationCountToNominate,
		TooLowCollatorNominationCountToNominate,
		TooLowNominationCountToLeaveNominators,
		InvalidBondConfig,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(crate) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Starting Block, Round, Number of Collators Selected, Total Balance
		NewRound {
			block: T::BlockNumber,
			round: RoundIndex,
			collators: u32,
			total_balance: BalanceOf<T>,
		},
		/// Account, Amount Locked, New Total Amt Locked
		JoinedCollatorCandidates {
			who: T::AccountId,
			amount_locked: BalanceOf<T>,
			total_locked: BalanceOf<T>,
		},
		/// Round, Collator Account, Total Exposed Amount (includes all
		/// nominations)
		CollatorChosen {
			round: RoundIndex,
			collator: T::AccountId,
			total_exposed_amount: BalanceOf<T>,
		},
		/// Collator Account, Old Bond, New Bond
		CollatorBondedMore {
			collator: T::AccountId,
			old_bond: BalanceOf<T>,
			new_bond: BalanceOf<T>,
		},
		/// Collator Account, Old Bond, New Bond
		CollatorBondedLess {
			collator: T::AccountId,
			old_bond: BalanceOf<T>,
			new_bond: BalanceOf<T>,
		},
		CollatorWentOffline {
			round: RoundIndex,
			collator: T::AccountId,
		},
		CollatorBackOnline {
			round: RoundIndex,
			collator: T::AccountId,
		},
		/// Round, Collator Account, Scheduled Exit
		CollatorScheduledExit {
			round: RoundIndex,
			collator: T::AccountId,
			scheduled_exit: RoundIndex,
		},
		/// Account, Amount Unlocked, New Total Amt Locked
		CollatorLeft {
			collator: T::AccountId,
			amount_unlocked: BalanceOf<T>,
			total_locked: BalanceOf<T>,
		},
		// Nominator, Collator, Amount, If in top nominations for collator after increase
		NominationIncreased {
			nominator: T::AccountId,
			collator: T::AccountId,
			amount: BalanceOf<T>,
			in_top: bool,
		},
		// Nominator, Collator, Amount, If in top nominations for collator after decrease
		NominationDecreased {
			nominator: T::AccountId,
			collator: T::AccountId,
			amount: BalanceOf<T>,
			in_top: bool,
		},
		/// Round, Nominator, Scheduled Exit
		NominatorExitScheduled {
			round: RoundIndex,
			nominator: T::AccountId,
			scheduled_exit: RoundIndex,
		},
		/// Round, Nominator, Collator, Scheduled Exit
		NominationRevocationScheduled {
			round: RoundIndex,
			nominator: T::AccountId,
			collator: T::AccountId,
			scheduled_exit: RoundIndex,
		},
		/// Nominator, Amount Unstaked
		NominatorLeft {
			nominator: T::AccountId,
			unstaked: BalanceOf<T>,
		},
		/// Nominator, Amount Locked, Collator, Nominator Position with New
		/// Total Counted if in Top
		Nomination {
			nominator: T::AccountId,
			locked: BalanceOf<T>,
			collator: T::AccountId,
			nominator_position: NominatorAdded<BalanceOf<T>>,
		},
		/// Nominator, Collator, Amount Unstaked, New Total Amt Staked for
		/// Collator
		NominatorLeftCollator {
			nominator: T::AccountId,
			collator: T::AccountId,
			unstaked: BalanceOf<T>,
			total_staked: BalanceOf<T>,
		},
		/// Paid the account (nominator or collator) the balance as liquid
		/// rewards
		Rewarded {
			who: T::AccountId,
			amount: BalanceOf<T>,
		},
		/// Transferred to account which holds funds reserved for parachain bond
		ReservedForParachainBond {
			who: T::AccountId,
			bond: BalanceOf<T>,
		},
		/// Account (re)set for parachain bond treasury [old, new]
		ParachainBondAccountSet {
			old: T::AccountId,
			new: T::AccountId,
		},
		/// Percent of inflation reserved for parachain bond (re)set [old, new]
		ParachainBondReservePercentSet {
			old: Percent,
			new: Percent,
		},
		/// Annual inflation input (first 3) was used to derive new per-round
		/// inflation (last 3)
		InflationSet {
			old_min: Perbill,
			old_ideal: Perbill,
			old_max: Perbill,
			new_min: Perbill,
			new_ideal: Perbill,
			new_max: Perbill,
		},
		/// Staking expectations set
		StakeExpectationsSet {
			min: BalanceOf<T>,
			ideal: BalanceOf<T>,
			max: BalanceOf<T>,
		},
		/// Set total selected candidates to this value [old, new]
		TotalSelectedSet {
			old: u32,
			new: u32,
		},
		/// Set collator commission to this value [old, new]
		CollatorCommissionSet {
			old: Perbill,
			new: Perbill,
		},
		/// Set blocks per round [current_round, first_block, old, new,
		/// new_per_round_inflation]
		BlocksPerRoundSet {
			round: RoundIndex,
			block: T::BlockNumber,
			old: u32,
			new: u32,
			min: Perbill,
			ideal: Perbill,
			max: Perbill,
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(n: T::BlockNumber) -> Weight {
			let mut round = <Round<T>>::get();

			if round.should_update(n) {
				// mutate round
				round.update(n);
				// pay all stakers for T::RewardPaymentDelay rounds ago
				Self::pay_stakers(round.current);
				// execute all delayed collator exits
				Self::execute_collator_exits(round.current);
				// execute all delayed nominator exits
				Self::execute_nominator_exits(round.current);
				// select top collator candidates for next round
				let (collator_count, nomination_count, total_staked) =
					Self::select_top_candidates(round.current);
				// start next round
				<Round<T>>::put(round);
				// snapshot total stake
				<Staked<T>>::insert(round.current, <Total<T>>::get());
				Self::deposit_event(Event::NewRound {
					block: round.first,
					round: round.current,
					collators: collator_count,
					total_balance: total_staked,
				});
				T::WeightInfo::active_on_initialize(collator_count, nomination_count)
			} else {
				T::WeightInfo::passive_on_initialize()
			}
		}
	}

	#[pallet::storage]
	#[pallet::getter(fn collator_commission)]
	/// Commission percent taken off of rewards for all collators
	type CollatorCommission<T: Config> = StorageValue<_, Perbill, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total_selected)]
	/// The total candidates selected every round
	type TotalSelected<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn parachain_bond_info)]
	/// Parachain bond config info { account, percent_of_inflation }
	type ParachainBondInfo<T: Config> =
		StorageValue<_, ParachainBondConfig<T::AccountId>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn round)]
	/// Current round index and next round scheduled transition
	type Round<T: Config> = StorageValue<_, RoundInfo<T::BlockNumber>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn nominator_state2)]
	/// Get nominator state associated with an account if account is nominating
	/// else None
	type NominatorState2<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		Nominator2<T::AccountId, BalanceOf<T>>,
		OptionQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn collator_state2)]
	/// Get collator state associated with an account if account is collating
	/// else None
	pub(crate) type CollatorState2<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		Collator2<T::AccountId, BalanceOf<T>>,
		OptionQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn selected_candidates)]
	/// The collator candidates selected for the current round
	type SelectedCandidates<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total)]
	/// Total capital locked by this staking pallet
	type Total<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn candidate_pool)]
	/// The pool of collator candidates, each with their total backing stake
	type CandidatePool<T: Config> =
		StorageValue<_, OrderedSet<Bond<T::AccountId, BalanceOf<T>>>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn exit_queue2)]
	/// A queue of collators and nominators awaiting exit
	type ExitQueue2<T: Config> = StorageValue<_, ExitQ<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn at_stake)]
	/// Snapshot of collator nomination stake at the start of the round
	pub type AtStake<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		RoundIndex,
		Twox64Concat,
		T::AccountId,
		CollatorSnapshot<T::AccountId, BalanceOf<T>>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn staked)]
	/// Total backing stake for selected candidates in the round
	pub type Staked<T: Config> = StorageMap<_, Twox64Concat, RoundIndex, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn inflation_config)]
	/// Inflation configuration
	pub type InflationConfig<T: Config> = StorageValue<_, InflationInfo<BalanceOf<T>>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn points)]
	/// Total points awarded to collators for block production in the round
	pub type Points<T: Config> = StorageMap<_, Twox64Concat, RoundIndex, RewardPoint, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn awarded_pts)]
	/// Points for each collator per round
	pub type AwardedPts<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		RoundIndex,
		Twox64Concat,
		T::AccountId,
		RewardPoint,
		ValueQuery,
	>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub candidates: Vec<(T::AccountId, BalanceOf<T>)>,
		pub nominations: Vec<(T::AccountId, T::AccountId, BalanceOf<T>)>,
		pub inflation_config: InflationInfo<BalanceOf<T>>,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { candidates: vec![], nominations: vec![], inflation_config: Default::default() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			<InflationConfig<T>>::put(self.inflation_config.clone());
			let mut candidate_count = 0u32;
			// Initialize the candidates
			for &(ref candidate, balance) in &self.candidates {
				assert!(
					T::Currency::free_balance(&candidate) >= balance,
					"Account does not have enough balance to bond as a candidate."
				);
				candidate_count += 1u32;
				if let Err(error) = <Pallet<T>>::join_candidates(
					T::Origin::from(Some(candidate.clone()).into()),
					balance,
					candidate_count,
				) {
					log::warn!("Join candidates failed in genesis with error {:?}", error);
				} else {
					candidate_count += 1u32;
				}
			}
			let mut col_nominator_count: BTreeMap<T::AccountId, u32> = BTreeMap::new();
			let mut nom_nominator_count: BTreeMap<T::AccountId, u32> = BTreeMap::new();
			// Initialize the nominations
			for &(ref nominator, ref target, balance) in &self.nominations {
				assert!(
					T::Currency::free_balance(&nominator) >= balance,
					"Account does not have enough balance to place nomination."
				);
				let cn_count =
					if let Some(x) = col_nominator_count.get(&target) { *x } else { 0u32 };
				let nn_count =
					if let Some(x) = nom_nominator_count.get(&nominator) { *x } else { 0u32 };
				if let Err(error) = <Pallet<T>>::nominate(
					T::Origin::from(Some(nominator.clone()).into()),
					target.clone(),
					balance,
					cn_count,
					nn_count,
				) {
					log::warn!("Join nominators failed in genesis with error {:?}", error);
				} else {
					if let Some(x) = col_nominator_count.get_mut(&target) {
						*x += 1u32;
					} else {
						col_nominator_count.insert(target.clone(), 1u32);
					};
					if let Some(x) = nom_nominator_count.get_mut(&nominator) {
						*x += 1u32;
					} else {
						nom_nominator_count.insert(nominator.clone(), 1u32);
					};
				}
			}
			// Set collator commission to default config
			<CollatorCommission<T>>::put(T::DefaultCollatorCommission::get());
			// Set parachain bond config to default config
			<ParachainBondInfo<T>>::put(ParachainBondConfig {
				account: Pallet::<T>::account_id(),
				percent: T::DefaultParachainBondReservePercent::get(),
			});

			// Set total selected candidates to minimum config
			<TotalSelected<T>>::put(T::MinSelectedCandidates::get());
			// Choose top TotalSelected collator candidates
			let (v_count, _, total_staked) = <Pallet<T>>::select_top_candidates(1u32);
			// Start Round 1 at Block 0
			let round: RoundInfo<T::BlockNumber> =
				RoundInfo::new(1u32, 0u32.into(), T::BlocksPerRound::get());
			<Round<T>>::put(round);
			// Snapshot total stake
			<Staked<T>>::insert(1u32, <Total<T>>::get());
			<Pallet<T>>::deposit_event(Event::NewRound {
				block: T::BlockNumber::zero(),
				round: 1u32,
				collators: v_count,
				total_balance: total_staked,
			});
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Set the expectations for total staked. These expectations determine
		/// the issuance for the round according to logic in `fn
		/// compute_issuance`
		#[pallet::weight(<T as Config>::WeightInfo::set_staking_expectations())]
		pub fn set_staking_expectations(
			origin: OriginFor<T>,
			expectations: Range<BalanceOf<T>>,
		) -> DispatchResultWithPostInfo {
			T::MonetaryGovernanceOrigin::ensure_origin(origin)?;
			ensure!(expectations.is_valid(), Error::<T>::InvalidSchedule);
			let mut config = <InflationConfig<T>>::get();
			ensure!(config.expect != expectations, Error::<T>::NoWritingSameValue);
			config.set_expectations(expectations);
			Self::deposit_event(Event::StakeExpectationsSet {
				min: config.expect.min,
				ideal: config.expect.ideal,
				max: config.expect.max,
			});
			<InflationConfig<T>>::put(config);
			Ok(().into())
		}

		/// Set the annual inflation rate to derive per-round inflation
		#[pallet::weight(<T as Config>::WeightInfo::set_inflation())]
		pub fn set_inflation(
			origin: OriginFor<T>,
			schedule: Range<Perbill>,
		) -> DispatchResultWithPostInfo {
			T::MonetaryGovernanceOrigin::ensure_origin(origin)?;
			ensure!(schedule.is_valid(), Error::<T>::InvalidSchedule);
			let mut config = <InflationConfig<T>>::get();
			ensure!(config.annual != schedule, Error::<T>::NoWritingSameValue);
			config.annual = schedule;
			config.set_round_from_annual::<T>(schedule);
			Self::deposit_event(Event::InflationSet {
				old_min: config.annual.min,
				old_ideal: config.annual.ideal,
				old_max: config.annual.max,
				new_min: config.round.min,
				new_ideal: config.round.ideal,
				new_max: config.round.max,
			});
			<InflationConfig<T>>::put(config);
			Ok(().into())
		}

		/// Set the account that will hold funds set aside for parachain bond
		#[pallet::weight(<T as Config>::WeightInfo::set_parachain_bond_account())]
		pub fn set_parachain_bond_account(
			origin: OriginFor<T>,
			new: T::AccountId,
		) -> DispatchResultWithPostInfo {
			T::MonetaryGovernanceOrigin::ensure_origin(origin)?;
			let ParachainBondConfig { account: old, percent } =
				<ParachainBondInfo<T>>::get().unwrap_or(Self::default_bond_config());
			ensure!(old != new, Error::<T>::NoWritingSameValue);
			<ParachainBondInfo<T>>::put(ParachainBondConfig { account: new.clone(), percent });
			Self::deposit_event(Event::ParachainBondAccountSet { old, new });

			Ok(().into())
		}

		/// Set the percent of inflation set aside for parachain bond
		#[pallet::weight(<T as Config>::WeightInfo::set_parachain_bond_reserve_percent())]
		pub fn set_parachain_bond_reserve_percent(
			origin: OriginFor<T>,
			new: Percent,
		) -> DispatchResultWithPostInfo {
			T::MonetaryGovernanceOrigin::ensure_origin(origin)?;
			let ParachainBondConfig { account, percent: old } =
				<ParachainBondInfo<T>>::get().unwrap_or(Self::default_bond_config());
			ensure!(old != new, Error::<T>::NoWritingSameValue);
			<ParachainBondInfo<T>>::put(ParachainBondConfig { account, percent: new });

			Self::deposit_event(Event::ParachainBondReservePercentSet { old, new });
			Ok(().into())
		}

		#[pallet::weight(<T as Config>::WeightInfo::set_total_selected())]
		/// Set the total number of collator candidates selected per round
		/// - changes are not applied until the start of the next round
		pub fn set_total_selected(origin: OriginFor<T>, new: u32) -> DispatchResultWithPostInfo {
			frame_system::ensure_root(origin)?;
			ensure!(new >= T::MinSelectedCandidates::get(), Error::<T>::CannotSetBelowMin);
			let old = <TotalSelected<T>>::get();
			ensure!(old != new, Error::<T>::NoWritingSameValue);
			<TotalSelected<T>>::put(new);
			Self::deposit_event(Event::TotalSelectedSet { old, new });
			Ok(().into())
		}

		#[pallet::weight(<T as Config>::WeightInfo::set_blocks_per_round())]
		/// Set blocks per round
		/// - if called with `new` less than length of current round, will
		///   transition immediately
		/// in the next block
		/// - also updates per-round inflation config
		pub fn set_blocks_per_round(origin: OriginFor<T>, new: u32) -> DispatchResultWithPostInfo {
			frame_system::ensure_root(origin)?;
			ensure!(new >= T::MinBlocksPerRound::get(), Error::<T>::CannotSetBelowMin);
			let mut round = <Round<T>>::get();
			let (now, first, old) = (round.current, round.first, round.length);
			ensure!(old != new, Error::<T>::NoWritingSameValue);
			round.length = new;
			// update per-round inflation given new rounds per year
			let mut inflation_config = <InflationConfig<T>>::get();
			inflation_config.reset_round(new);
			<Round<T>>::put(round);
			Self::deposit_event(Event::BlocksPerRoundSet {
				round: now,
				block: first,
				old,
				new,
				min: inflation_config.round.min,
				ideal: inflation_config.round.ideal,
				max: inflation_config.round.max,
			});
			<InflationConfig<T>>::put(inflation_config);
			Ok(().into())
		}

		#[pallet::weight(<T as Config>::WeightInfo::set_collator_commission())]
		/// Set the commission for all collators
		pub fn set_collator_commission(
			origin: OriginFor<T>,
			new: Perbill,
		) -> DispatchResultWithPostInfo {
			frame_system::ensure_root(origin)?;
			let old = <CollatorCommission<T>>::get();
			ensure!(old != new, Error::<T>::NoWritingSameValue);
			<CollatorCommission<T>>::put(new);
			Self::deposit_event(Event::CollatorCommissionSet { old, new });
			Ok(().into())
		}

		/// Join the set of collator candidates
		#[pallet::weight(<T as Config>::WeightInfo::join_candidates(*candidate_count))]
		pub fn join_candidates(
			origin: OriginFor<T>,
			bond: BalanceOf<T>,
			candidate_count: u32,
		) -> DispatchResultWithPostInfo {
			let acc = ensure_signed(origin)?;
			ensure!(!Self::is_candidate(&acc), Error::<T>::CandidateExists);
			ensure!(!Self::is_nominator(&acc), Error::<T>::NominatorExists);
			ensure!(bond >= T::MinCollatorCandidateStk::get(), Error::<T>::ValBondBelowMin);
			let mut candidates = <CandidatePool<T>>::get();
			let old_count = candidates.0.len() as u32;
			ensure!(
				candidate_count >= old_count,
				Error::<T>::TooLowCandidateCountWeightHintJoinCandidates
			);
			ensure!(
				candidates.insert(Bond { owner: acc.clone(), amount: bond }),
				Error::<T>::CandidateExists
			);
			T::Currency::reserve(&acc, bond)?;
			let candidate = Collator2::new(acc.clone(), bond);
			<CollatorState2<T>>::insert(&acc, candidate);
			<CandidatePool<T>>::put(candidates);
			let new_total = <Total<T>>::get().saturating_add(bond);
			<Total<T>>::put(new_total);
			Self::deposit_event(Event::JoinedCollatorCandidates {
				who: acc,
				amount_locked: bond,
				total_locked: new_total,
			});
			Ok(().into())
		}

		/// Request to leave the set of candidates. If successful, the account
		/// is immediately removed from the candidate pool to prevent selection
		/// as a collator, but unbonding is executed with a delay of
		/// `T::LeaveCandidates` rounds.
		#[pallet::weight(<T as Config>::WeightInfo::leave_candidates(*candidate_count))]
		pub fn leave_candidates(
			origin: OriginFor<T>,
			candidate_count: u32,
		) -> DispatchResultWithPostInfo {
			let collator = ensure_signed(origin)?;
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(!state.is_leaving(), Error::<T>::CandidateAlreadyLeaving);
			let mut exits = <ExitQueue2<T>>::get();
			let now = <Round<T>>::get().current;
			let when = now + T::LeaveCandidatesDelay::get();
			exits.schedule_candidate_exit::<T>(collator.clone(), when)?;
			state.leave(when);
			let mut candidates = <CandidatePool<T>>::get();
			ensure!(
				candidate_count >= candidates.0.len() as u32,
				Error::<T>::TooLowCollatorCandidateCountToLeaveCandidates
			);
			if candidates.remove(&Bond::from_owner(collator.clone())) {
				<CandidatePool<T>>::put(candidates);
			}
			<ExitQueue2<T>>::put(exits);
			<CollatorState2<T>>::insert(&collator, state);
			Self::deposit_event(Event::CollatorScheduledExit {
				round: now,
				collator,
				scheduled_exit: when,
			});
			Ok(().into())
		}

		/// Temporarily leave the set of collator candidates without unbonding
		#[pallet::weight(<T as Config>::WeightInfo::go_offline())]
		pub fn go_offline(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			let collator = ensure_signed(origin)?;
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(state.is_active(), Error::<T>::AlreadyOffline);
			state.go_offline();
			let mut candidates = <CandidatePool<T>>::get();
			if candidates.remove(&Bond::from_owner(collator.clone())) {
				<CandidatePool<T>>::put(candidates);
			}
			<CollatorState2<T>>::insert(&collator, state);
			Self::deposit_event(Event::CollatorWentOffline {
				round: <Round<T>>::get().current,
				collator,
			});
			Ok(().into())
		}

		/// Rejoin the set of collator candidates if previously had called
		/// `go_offline`
		#[pallet::weight(<T as Config>::WeightInfo::go_online())]
		pub fn go_online(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			let collator = ensure_signed(origin)?;
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(!state.is_active(), Error::<T>::AlreadyActive);
			ensure!(!state.is_leaving(), Error::<T>::CannotActBecauseLeaving);
			state.go_online();
			let mut candidates = <CandidatePool<T>>::get();
			ensure!(
				candidates.insert(Bond { owner: collator.clone(), amount: state.total_counted }),
				Error::<T>::AlreadyActive
			);
			<CandidatePool<T>>::put(candidates);
			<CollatorState2<T>>::insert(&collator, state);
			Self::deposit_event(Event::CollatorBackOnline {
				round: <Round<T>>::get().current,
				collator,
			});
			Ok(().into())
		}

		/// Bond more for collator candidates
		#[pallet::weight(<T as Config>::WeightInfo::candidate_bond_more())]
		pub fn candidate_bond_more(
			origin: OriginFor<T>,
			more: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let collator = ensure_signed(origin)?;
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(!state.is_leaving(), Error::<T>::CannotActBecauseLeaving);
			T::Currency::reserve(&collator, more)?;
			let before = state.bond;
			state.bond_more(more);
			let after = state.bond;
			if state.is_active() {
				Self::update_active(collator.clone(), state.total_counted);
			}
			<CollatorState2<T>>::insert(&collator, state);
			let new_total = <Total<T>>::get().saturating_add(more);
			<Total<T>>::put(new_total);
			Self::deposit_event(Event::CollatorBondedMore {
				collator,
				old_bond: before,
				new_bond: after,
			});
			Ok(().into())
		}

		/// Bond less for collator candidates
		#[pallet::weight(<T as Config>::WeightInfo::candidate_bond_less())]
		pub fn candidate_bond_less(
			origin: OriginFor<T>,
			less: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let collator = ensure_signed(origin)?;
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(!state.is_leaving(), Error::<T>::CannotActBecauseLeaving);
			let before = state.bond;
			let after = state.bond_less(less).ok_or(Error::<T>::ValBondBelowMin)?;
			ensure!(after >= T::MinCollatorCandidateStk::get(), Error::<T>::ValBondBelowMin);
			T::Currency::unreserve(&collator, less);
			if state.is_active() {
				Self::update_active(collator.clone(), state.total_counted);
			}
			<CollatorState2<T>>::insert(&collator, state);
			let new_total_staked = <Total<T>>::get().saturating_sub(less);
			<Total<T>>::put(new_total_staked);
			Self::deposit_event(Event::CollatorBondedLess {
				collator,
				old_bond: before,
				new_bond: after,
			});
			Ok(().into())
		}

		/// If caller is not a nominator, then join the set of nominators
		/// If caller is a nominator, then makes nomination to change their
		/// nomination state
		#[pallet::weight(
			<T as Config>::WeightInfo::nominate(
				*collator_nominator_count,
				*nomination_count
			)
		)]
		pub fn nominate(
			origin: OriginFor<T>,
			collator: T::AccountId,
			amount: BalanceOf<T>,
			collator_nominator_count: u32,
			nomination_count: u32,
		) -> DispatchResultWithPostInfo {
			let acc = ensure_signed(origin)?;
			let nominator = if let Some(mut state) = <NominatorState2<T>>::get(&acc) {
				ensure!(state.is_active(), Error::<T>::CannotActBecauseLeaving);
				// nomination after first
				ensure!(amount >= T::MinNomination::get(), Error::<T>::NominationBelowMin);
				ensure!(
					nomination_count >= state.nominations.0.len() as u32,
					Error::<T>::TooLowNominationCountToNominate
				);
				ensure!(
					(state.nominations.0.len() as u32) < T::MaxCollatorsPerNominator::get(),
					Error::<T>::ExceedMaxCollatorsPerNom
				);
				// ensure that nominator is not in the exit_queue
				ensure!(
					state.add_nomination(Bond { owner: collator.clone(), amount }),
					Error::<T>::AlreadyNominatedCollator
				);
				state
			} else {
				// first nomination
				ensure!(amount >= T::MinNominatorStk::get(), Error::<T>::NomBondBelowMin);
				ensure!(!Self::is_candidate(&acc), Error::<T>::CandidateExists);
				Nominator2::new(collator.clone(), amount)
			};
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(
				collator_nominator_count >= state.nominators.0.len() as u32,
				Error::<T>::TooLowCollatorNominationCountToNominate
			);
			let nominator_position = state.add_nominator::<T>(acc.clone(), amount)?;
			T::Currency::reserve(&acc, amount)?;
			if let NominatorAdded::AddedToTop { new_total } = nominator_position {
				if state.is_active() {
					Self::update_active(collator.clone(), new_total);
				}
			}
			let new_total_locked = <Total<T>>::get() + amount;
			<Total<T>>::put(new_total_locked);
			<CollatorState2<T>>::insert(&collator, state);
			<NominatorState2<T>>::insert(&acc, nominator);
			Self::deposit_event(Event::Nomination {
				nominator: acc,
				locked: amount,
				collator,
				nominator_position,
			});
			Ok(().into())
		}

		/// Request to leave the set of nominators. If successful, the nominator
		/// is scheduled to exit
		#[pallet::weight(<T as Config>::WeightInfo::leave_nominators(*nomination_count))]
		pub fn leave_nominators(
			origin: OriginFor<T>,
			nomination_count: u32,
		) -> DispatchResultWithPostInfo {
			let acc = ensure_signed(origin)?;
			let mut state = <NominatorState2<T>>::get(&acc).ok_or(Error::<T>::NominatorDNE)?;
			ensure!(!state.is_leaving(), Error::<T>::NominatorAlreadyLeaving);
			ensure!(
				nomination_count >= (state.nominations.0.len() as u32),
				Error::<T>::TooLowNominationCountToLeaveNominators
			);
			let mut exits = <ExitQueue2<T>>::get();
			let now = <Round<T>>::get().current;
			let when = now + T::LeaveNominatorsDelay::get();
			exits.schedule_nominator_exit::<T>(acc.clone(), when)?;
			state.leave(when);
			state.scheduled_revocations_total = state.total;
			state.scheduled_revocations_count = state.nominations.0.len() as u32;
			<ExitQueue2<T>>::put(exits);
			<NominatorState2<T>>::insert(&acc, state);
			Self::deposit_event(Event::NominatorExitScheduled {
				round: now,
				nominator: acc,
				scheduled_exit: when,
			});
			Ok(().into())
		}

		/// Request to revoke an existing nomination. If successful, the
		/// nomination is scheduled to exit
		#[pallet::weight(<T as Config>::WeightInfo::revoke_nomination())]
		pub fn revoke_nomination(
			origin: OriginFor<T>,
			collator: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let nominator = ensure_signed(origin)?;
			let mut state =
				<NominatorState2<T>>::get(&nominator).ok_or(Error::<T>::NominatorDNE)?;
			ensure!(state.is_active(), Error::<T>::CannotActBecauseLeaving);
			ensure!(
				state.revocations.insert(collator.clone()),
				Error::<T>::NominationAlreadyRevoked
			);
			let mut nomination_amount: Option<BalanceOf<T>> = None;
			for Bond { owner, amount } in state.nominations.0.iter() {
				if owner == &collator {
					nomination_amount = Some(*amount);
					break
				}
			}
			// Ensure that the collator exists in the nominations
			let amount = nomination_amount.ok_or(Error::<T>::NominationDNE)?;
			let remaining = state.total - state.scheduled_revocations_total - amount;
			let leaving =
				if state.nominations.0.len() as u32 - state.scheduled_revocations_count < 2 {
					true
				} else {
					ensure!(remaining >= T::MinNominatorStk::get(), Error::<T>::NomBondBelowMin);
					false
				};
			let mut exits = <ExitQueue2<T>>::get();
			let now = <Round<T>>::get().current;
			let when = now + T::RevokeNominationDelay::get();
			if leaving {
				// schedule to leave the set of nominators if this is the only nomination
				exits.schedule_nominator_exit::<T>(nominator.clone(), when)?;
				state.leave(when);
				state.scheduled_revocations_total = state.total;
				state.scheduled_revocations_count = state.nominations.0.len() as u32;
				<ExitQueue2<T>>::put(exits);
				<NominatorState2<T>>::insert(&nominator, state);
				Self::deposit_event(Event::NominatorExitScheduled {
					round: now,
					nominator,
					scheduled_exit: when,
				});
			} else {
				// schedule to revoke this nomination
				exits.schedule_nomination_revocation::<T>(
					nominator.clone(),
					collator.clone(),
					when,
				)?;
				state.scheduled_revocations_total += amount;
				state.scheduled_revocations_count += 1u32;
				<ExitQueue2<T>>::put(exits);
				<NominatorState2<T>>::insert(&nominator, state);
				Self::deposit_event(Event::NominationRevocationScheduled {
					round: now,
					nominator,
					collator,
					scheduled_exit: when,
				});
			}
			Ok(().into())
		}

		/// Bond more for nominators with respect to a specific collator
		/// candidate
		#[pallet::weight(<T as Config>::WeightInfo::nominator_bond_more())]
		pub fn nominator_bond_more(
			origin: OriginFor<T>,
			candidate: T::AccountId,
			more: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let nominator = ensure_signed(origin)?;
			let mut state =
				<NominatorState2<T>>::get(&nominator).ok_or(Error::<T>::NominatorDNE)?;
			ensure!(state.is_active(), Error::<T>::CannotActBecauseLeaving);
			ensure!(!state.revocations.contains(&candidate), Error::<T>::CannotActBecauseRevoking);
			let mut collator =
				<CollatorState2<T>>::get(&candidate).ok_or(Error::<T>::CandidateDNE)?;
			ensure!(state.inc_nomination(candidate.clone(), more), Error::<T>::NominationDNE);
			T::Currency::reserve(&nominator, more)?;
			let before = collator.total_counted;
			let in_top = collator.inc_nominator(nominator.clone(), more);
			let after = collator.total_counted;
			if collator.is_active() && (before != after) {
				Self::update_active(candidate.clone(), after);
			}
			<CollatorState2<T>>::insert(&candidate, collator);
			<NominatorState2<T>>::insert(&nominator, state);
			let new_total_staked = <Total<T>>::get().saturating_add(more);
			<Total<T>>::put(new_total_staked);
			Self::deposit_event(Event::NominationIncreased {
				nominator,
				collator: candidate,
				amount: more,
				in_top,
			});
			Ok(().into())
		}

		/// Bond less for nominators with respect to a specific nominator
		/// candidate
		#[pallet::weight(<T as Config>::WeightInfo::nominator_bond_less())]
		pub fn nominator_bond_less(
			origin: OriginFor<T>,
			candidate: T::AccountId,
			less: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let nominator = ensure_signed(origin)?;
			let mut state =
				<NominatorState2<T>>::get(&nominator).ok_or(Error::<T>::NominatorDNE)?;
			ensure!(state.is_active(), Error::<T>::CannotActBecauseLeaving);
			ensure!(!state.revocations.contains(&candidate), Error::<T>::CannotActBecauseRevoking);
			let mut collator =
				<CollatorState2<T>>::get(&candidate).ok_or(Error::<T>::CandidateDNE)?;
			let remaining = state
				.dec_nomination(candidate.clone(), less)
				.ok_or(Error::<T>::NominationDNE)?
				.ok_or(Error::<T>::NomBondBelowMin)?;
			ensure!(remaining >= T::MinNomination::get(), Error::<T>::NominationBelowMin);
			ensure!(state.total >= T::MinNominatorStk::get(), Error::<T>::NomBondBelowMin);
			T::Currency::unreserve(&nominator, less);
			let before = collator.total_counted;
			let in_top = collator.dec_nominator(nominator.clone(), less);
			let after = collator.total_counted;
			if collator.is_active() && (before != after) {
				Self::update_active(candidate.clone(), after);
			}
			<CollatorState2<T>>::insert(&candidate, collator);
			<NominatorState2<T>>::insert(&nominator, state);
			let new_total_staked = <Total<T>>::get().saturating_sub(less);
			<Total<T>>::put(new_total_staked);
			Self::deposit_event(Event::NominationDecreased {
				nominator,
				collator: candidate,
				amount: less,
				in_top,
			});
			Ok(().into())
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account()
		}

		pub fn default_bond_config() -> ParachainBondConfig<T::AccountId> {
			ParachainBondConfig {
				account: Self::account_id(),
				percent: T::DefaultParachainBondReservePercent::get(),
			}
		}

		pub fn is_nominator(acc: &T::AccountId) -> bool {
			<NominatorState2<T>>::get(acc).is_some()
		}

		pub fn is_candidate(acc: &T::AccountId) -> bool {
			<CollatorState2<T>>::get(acc).is_some()
		}

		pub fn is_selected_candidate(acc: &T::AccountId) -> bool {
			<SelectedCandidates<T>>::get().binary_search(acc).is_ok()
		}

		// ensure candidate is active before calling
		fn update_active(candidate: T::AccountId, total: BalanceOf<T>) {
			let mut candidates = <CandidatePool<T>>::get();
			candidates.remove(&Bond::from_owner(candidate.clone()));
			candidates.insert(Bond { owner: candidate, amount: total });
			<CandidatePool<T>>::put(candidates);
		}

		// Calculate round issuance based on total staked for the given round
		fn compute_issuance(staked: BalanceOf<T>) -> BalanceOf<T> {
			let config = <InflationConfig<T>>::get();
			let round_issuance = crate::inflation::round_issuance_range::<T>(config.round);
			// TODO: consider interpolation instead of bounded range
			if staked < config.expect.min {
				round_issuance.min
			} else if staked > config.expect.max {
				round_issuance.max
			} else {
				round_issuance.ideal
			}
		}

		pub(crate) fn nominator_leaves_collator(
			nominator: T::AccountId,
			collator: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let mut state = <CollatorState2<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
			let (total_changed, nominator_stake) = state.rm_nominator::<T>(nominator.clone())?;
			T::Currency::unreserve(&nominator, nominator_stake);
			if state.is_active() && total_changed {
				Self::update_active(collator.clone(), state.total_counted);
			}
			let new_total_locked = <Total<T>>::get() - nominator_stake;
			<Total<T>>::put(new_total_locked);
			let new_total = state.total_counted;
			<CollatorState2<T>>::insert(&collator, state);
			Self::deposit_event(Event::NominatorLeftCollator {
				nominator,
				collator,
				unstaked: nominator_stake,
				total_staked: new_total,
			});
			Ok(().into())
		}

		fn pay_stakers(next: RoundIndex) {
			// payout is next - duration rounds ago => next - duration > 0 else return early
			let duration = T::RewardPaymentDelay::get();
			if next <= duration {
				return
			}
			let round_to_payout = next - duration;
			let total = <Points<T>>::get(round_to_payout);
			if total.is_zero() {
				return
			}
			let total_staked = <Staked<T>>::get(round_to_payout);
			let total_issuance = Self::compute_issuance(total_staked);
			let mut left_issuance = total_issuance;
			// reserve portion of issuance for parachain bond account
			let bond_config = <ParachainBondInfo<T>>::get().unwrap_or(Self::default_bond_config());
			let parachain_bond_reserve = bond_config.percent * total_issuance;
			if let Ok(imb) =
				T::Currency::deposit_into_existing(&bond_config.account, parachain_bond_reserve)
			{
				// update round issuance iff transfer succeeds
				left_issuance -= imb.peek();
				Self::deposit_event(Event::ReservedForParachainBond {
					who: bond_config.account,
					bond: imb.peek(),
				});
			}
			let mint = |amt: BalanceOf<T>, to: T::AccountId| {
				if let Ok(imb) = T::Currency::deposit_into_existing(&to, amt) {
					Self::deposit_event(Event::Rewarded { who: to.clone(), amount: imb.peek() });
				}
			};
			// only pay out rewards at the end to transfer only total amount due
			let mut due_rewards: BTreeMap<T::AccountId, BalanceOf<T>> = BTreeMap::new();
			let mut increase_due_rewards = |amt: BalanceOf<T>, to: T::AccountId| {
				if let Some(already_due) = due_rewards.get(&to) {
					let amount = amt.saturating_add(*already_due);
					due_rewards.insert(to, amount);
				} else {
					due_rewards.insert(to, amt);
				}
			};
			let collator_fee = <CollatorCommission<T>>::get();
			let collator_issuance = collator_fee * total_issuance;
			for (val, pts) in <AwardedPts<T>>::drain_prefix(round_to_payout) {
				let pct_due = Perbill::from_rational(pts, total);
				let mut amt_due = pct_due * left_issuance;
				// Take the snapshot of block author and nominations
				let state = <AtStake<T>>::take(round_to_payout, &val);
				if state.nominators.is_empty() {
					// solo collator with no nominators
					mint(amt_due, val.clone());
				} else {
					// pay collator first; commission + due_portion
					let val_pct = Perbill::from_rational(state.bond, state.total);
					let commission = pct_due * collator_issuance;
					amt_due -= commission;
					let val_due = (val_pct * amt_due) + commission;
					mint(val_due, val.clone());
					// pay nominators due portion
					for Bond { owner, amount } in state.nominators {
						let percent = Perbill::from_rational(amount, state.total);
						let due = percent * amt_due;
						increase_due_rewards(due, owner);
					}
				}
			}
			for (nominator, total_due) in due_rewards {
				mint(total_due, nominator);
			}
		}

		/// Executes all collator exits scheduled for when <= now
		fn execute_collator_exits(now: RoundIndex) {
			let mut exit_queue = <ExitQueue2<T>>::get();
			let remaining_exits = exit_queue
				.candidate_schedule
				.clone()
				.into_iter()
				.filter_map(|(who, when)| {
					if when > now {
						Some((who, when))
					} else {
						if !exit_queue.candidates.remove(&who) {
							log::warn!(
								"Candidates set removal failed, CollatorState had inconsistency!",
							);
						}
						if let Some(state) = <CollatorState2<T>>::get(&who) {
							// return stake to nominator
							let return_stake = |bond: Bond<T::AccountId, BalanceOf<T>>| {
								T::Currency::unreserve(&bond.owner, bond.amount);
								// remove nomination from nominator state
								let mut nominator = NominatorState2::<T>::get(&bond.owner).expect(
									"Collator state and nominator state are consistent. 
										Collator state has a record of this nomination. Therefore, 
										Nominator state also has a record. qed.",
								);
								if let Some(remaining) = nominator.rm_nomination(who.clone()) {
									if remaining.is_zero() {
										<NominatorState2<T>>::remove(&bond.owner);
									} else {
										<NominatorState2<T>>::insert(&bond.owner, nominator);
									}
								}
							};
							// return all top nominations
							for bond in state.top_nominators {
								return_stake(bond);
							}
							// return all bottom nominations
							for bond in state.bottom_nominators {
								return_stake(bond);
							}
							// return stake to collator
							T::Currency::unreserve(&state.id, state.bond);
							<CollatorState2<T>>::remove(&who);
							let new_total_staked =
								<Total<T>>::get().saturating_sub(state.total_backing);
							<Total<T>>::put(new_total_staked);
							Self::deposit_event(Event::CollatorLeft {
								collator: who,
								amount_unlocked: state.total_backing,
								total_locked: new_total_staked,
							});
						}
						None
					}
				})
				.collect::<Vec<(T::AccountId, RoundIndex)>>();
			exit_queue.candidate_schedule = remaining_exits;
			<ExitQueue2<T>>::put(exit_queue);
		}

		/// Executes all nominator exits for when <= now
		fn execute_nominator_exits(now: RoundIndex) {
			let mut exit_queue = <ExitQueue2<T>>::get();
			let remaining_exits = exit_queue
				.nominator_schedule
				.clone()
				.into_iter()
				.filter_map(|(nominator, maybe_collator, when)| {
					if when > now {
						Some((nominator, maybe_collator, when))
					} else {
						if let Some(collator) = maybe_collator {
							// single revocation needs to be executed
							if let Some(mut state) = <NominatorState2<T>>::get(&nominator) {
								let pre_total = state.total;
								if let Some(remaining) = state.rm_nomination(collator.clone()) {
									let amount = pre_total - remaining;
									state.scheduled_revocations_total -= amount;
									state.scheduled_revocations_count -= 1u32;
									state.revocations.remove(&collator);
									let _ = Self::nominator_leaves_collator(
										nominator.clone(),
										collator,
									);
									<NominatorState2<T>>::insert(&nominator, state);
								}
							} else {
								log::warn!(
									"Nominator State for Nominator {:?} Not Found During Revocation 
									of Support for Collator {:?}",
									nominator,
									collator,
								);
							}
						} else {
							if !exit_queue.nominators_leaving.remove(&nominator) {
								log::warn!(
									"Nominators set removal failed,
									NominatorState had inconsistency!",
								);
							}
							if let Some(state) = <NominatorState2<T>>::get(&nominator) {
								for bond in state.nominations.0 {
									if let Err(error) = Self::nominator_leaves_collator(
										nominator.clone(),
										bond.owner.clone(),
									) {
										log::warn!(
											"Nominator leaves collator failed with error: {:?}",
											error
										);
									}
								}
								<NominatorState2<T>>::remove(&nominator);
								Self::deposit_event(Event::NominatorLeft {
									nominator,
									unstaked: state.total,
								});
							} else {
								log::warn!(
									"Nominator State Not Found During Exit for Nominator {:?}",
									nominator
								);
							}
						}
						None
					}
				})
				.collect::<Vec<(T::AccountId, Option<T::AccountId>, RoundIndex)>>();
			exit_queue.nominator_schedule = remaining_exits;
			<ExitQueue2<T>>::put(exit_queue);
		}

		/// Compute the top `TotalSelected` candidates in the CandidatePool and
		/// return a vec of their AccountIds (in the order of selection)
		pub fn compute_top_candidates() -> Vec<T::AccountId> {
			let mut candidates = <CandidatePool<T>>::get().0;
			// order candidates by stake (least to greatest so requires `rev()`)
			candidates.sort_unstable_by(|a, b| a.amount.partial_cmp(&b.amount).unwrap());
			let top_n = <TotalSelected<T>>::get() as usize;
			// choose the top TotalSelected qualified candidates, ordered by stake
			let mut collators = candidates
				.into_iter()
				.rev()
				.take(top_n)
				.filter(|x| x.amount >= T::MinCollatorStk::get())
				.map(|x| x.owner)
				.collect::<Vec<T::AccountId>>();
			collators.sort();
			collators
		}

		/// Best as in most cumulatively supported in terms of stake
		/// Returns [collators, collator_count, nomination_count, total staked]
		fn select_top_candidates(next: RoundIndex) -> (u32, u32, BalanceOf<T>) {
			let (mut collator_count, mut nomination_count, mut total) =
				(0u32, 0u32, BalanceOf::<T>::zero());
			// choose the top TotalSelected qualified candidates, ordered by stake
			let collators = Self::compute_top_candidates();
			// snapshot exposure for round for weighting amount distribution
			for account in collators.iter() {
				let state = <CollatorState2<T>>::get(&account)
					.expect("all members of CandidateQ must be candidates");
				collator_count += 1u32;
				nomination_count += state.nominators.0.len() as u32;
				let amount = state.total_counted;
				total += amount;
				let exposure: CollatorSnapshot<T::AccountId, BalanceOf<T>> = state.into();
				<AtStake<T>>::insert(next, account, exposure);
				Self::deposit_event(Event::CollatorChosen {
					round: next,
					collator: account.clone(),
					total_exposed_amount: amount,
				});
			}
			// insert canonical collator set
			<SelectedCandidates<T>>::put(collators);
			(collator_count, nomination_count, total)
		}
	}

	// Add amount points to block authors:
	// * 20 points to the block producer for producing a block in the chain
	impl<T: Config> pallet_authorship::EventHandler<T::AccountId, T::BlockNumber> for Pallet<T> {
		fn note_author(author: T::AccountId) {
			let now = <Round<T>>::get().current;
			let score_plus_20 = <AwardedPts<T>>::get(now, &author) + 20;
			<AwardedPts<T>>::insert(now, author, score_plus_20);
			<Points<T>>::mutate(now, |x| *x += 20);

			// TODO Benchmark this function
		}

		fn note_uncle(_author: T::AccountId, _age: T::BlockNumber) {}
	}

	impl<T: Config> SessionManager<T::AccountId> for Pallet<T> {
		fn new_session(index: SessionIndex) -> Option<Vec<T::AccountId>> {
			let block_number = <frame_system::Pallet<T>>::block_number();

			log::info!("assembling new collators for new session {} at #{:?}", index, block_number,);

			Some(Self::selected_candidates())

			// TODO Benchmark this function
		}

		fn new_session_genesis(_new_index: SessionIndex) -> Option<Vec<T::AccountId>> {
			let collators = Self::selected_candidates();

			if collators.is_empty() {
				return None
			}

			Some(collators)

			// TODO Benchmark this function
		}

		fn start_session(_: SessionIndex) {
			// we don't care.
		}

		fn end_session(_: SessionIndex) {
			// we don't care.
		}
	}

	impl<T: Config> Get<Vec<T::AccountId>> for Pallet<T> {
		fn get() -> Vec<T::AccountId> {
			Self::selected_candidates()
		}
	}

	impl<T: Config> ShouldEndSession<T::BlockNumber> for Pallet<T> {
		fn should_end_session(now: T::BlockNumber) -> bool {
			let round = <Round<T>>::get();
			if (round.first == 0u32.into()) & (now == 0u32.into()) {
				return false
			}
			round.first == now
		}
	}

	impl<T: Config> EstimateNextSessionRotation<T::BlockNumber> for Pallet<T> {
		fn average_session_length() -> T::BlockNumber {
			let round = <Round<T>>::get();
			round.length.into()
		}

		fn estimate_current_session_progress(now: T::BlockNumber) -> (Option<Permill>, Weight) {
			let round = <Round<T>>::get();
			let period: T::BlockNumber = T::BlocksPerRound::get().into();

			// NOTE: we add one since we assume that the current block has already elapsed,
			// i.e. when evaluating the last block in the session the progress should be
			// 100% (0% is never returned).
			let progress = if now >= round.first {
				let current = (now - round.first) % period.clone() + One::one();
				Some(Permill::from_rational(current.clone(), period.clone()))
			} else {
				Some(Permill::from_rational(now + One::one(), round.first))
			};

			// TODO Benchmark this function, weight should not be zero
			(progress, Zero::zero())
		}

		fn estimate_next_session_rotation(now: T::BlockNumber) -> (Option<T::BlockNumber>, Weight) {
			let round = <Round<T>>::get();
			let offset = round.first;
			let period: T::BlockNumber = T::BlocksPerRound::get().into();

			let next_session = if now > offset {
				let block_after_last_session = (now.clone() - offset) % period.clone();
				if block_after_last_session > Zero::zero() {
					now.saturating_add(period.saturating_sub(block_after_last_session))
				} else {
					// this branch happens when the session is already rotated or will rotate in
					// this block (depending on being called before or after
					// `session::on_initialize`). Here, we assume the latter, namely that this is
					// called after `session::on_initialize`, and thus we add period to it as well.
					now + period
				}
			} else {
				offset
			};

			// TODO Benchmark this function, weight should not be zero
			(Some(next_session), Zero::zero())
		}
	}
}
