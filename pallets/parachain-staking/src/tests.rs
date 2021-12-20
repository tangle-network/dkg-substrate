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

//! # Staking Pallet Unit Tests
//! The unit tests are organized by the call they test. The order matches the
//! order of the calls in the `lib.rs`.
//! 1. Root
//! 2. Monetary Governance
//! 3. Public (Collator, Nominator)
//! 4. Miscellaneous Property-Based Tests
use crate::{
	mock::{
		events, last_event, roll_to, set_author, Balances, Event as MetaEvent, ExtBuilder, Origin,
		Stake, Test,
	},
	Bond, CollatorStatus, Error, Event, NominatorAdded, Pallet, Range,
};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::{traits::Zero, DispatchError, Perbill, Percent};

/// Prints the diff iff assert_eq fails, should only be used for debugging
/// purposes
#[macro_export]
macro_rules! asserts_eq {
	($left:expr, $right:expr) => {
		match (&$left, &$right) {
			(left_val, right_val) => {
				similar_asserts::assert_eq!(*left_val, *right_val);
			},
		}
	};
}

// ~~ ROOT ~~

#[test]
fn invalid_root_origin_fails() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_total_selected(Origin::signed(45), 6u32),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Stake::set_collator_commission(Origin::signed(45), Perbill::from_percent(5)),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

// SET TOTAL SELECTED

#[test]
fn set_total_selected_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_total_selected(Origin::root(), 6u32));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::TotalSelectedSet { old: 5u32, new: 6u32 })
		);
	});
}

#[test]
fn set_total_selected_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::total_selected(), 5u32);
		assert_ok!(Stake::set_total_selected(Origin::root(), 6u32));
		assert_eq!(Stake::total_selected(), 6u32);
	});
}

#[test]
fn cannot_set_total_selected_to_current_total_selected() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_total_selected(Origin::root(), 5u32),
			Error::<Test>::NoWritingSameValue
		);
	});
}

#[test]
fn cannot_set_total_selected_below_module_min() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_total_selected(Origin::root(), 4u32),
			Error::<Test>::CannotSetBelowMin
		);
	});
}

// SET COLLATOR COMMISSION

#[test]
fn set_collator_commission_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_collator_commission(Origin::root(), Perbill::from_percent(5)));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::CollatorCommissionSet {
				old: Perbill::from_percent(20),
				new: Perbill::from_percent(5),
			})
		);
	});
}

#[test]
fn set_collator_commission_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::collator_commission(), Perbill::from_percent(20));
		assert_ok!(Stake::set_collator_commission(Origin::root(), Perbill::from_percent(5)));
		assert_eq!(Stake::collator_commission(), Perbill::from_percent(5));
	});
}

#[test]
fn cannot_set_collator_commission_to_current_collator_commission() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_collator_commission(Origin::root(), Perbill::from_percent(20)),
			Error::<Test>::NoWritingSameValue
		);
	});
}

// SET BLOCKS PER ROUND

#[test]
fn set_blocks_per_round_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_blocks_per_round(Origin::root(), 3u32));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::BlocksPerRoundSet {
				round: 1,
				block: 0,
				old: 5,
				new: 3,
				min: Perbill::from_parts(463),
				ideal: Perbill::from_parts(463),
				max: Perbill::from_parts(463)
			})
		);
	});
}

#[test]
fn set_blocks_per_round_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::round().length, 5);
		assert_ok!(Stake::set_blocks_per_round(Origin::root(), 3u32));
		assert_eq!(Stake::round().length, 3);
	});
}

#[test]
fn cannot_set_blocks_per_round_below_module_min() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_blocks_per_round(Origin::root(), 2u32),
			Error::<Test>::CannotSetBelowMin
		);
	});
}

#[test]
fn cannot_set_blocks_per_round_to_current_blocks_per_round() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_blocks_per_round(Origin::root(), 5u32),
			Error::<Test>::NoWritingSameValue
		);
	});
}

#[test]
fn round_immediately_jumps_if_current_duration_exceeds_new_blocks_per_round() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			// default round every 5 blocks
			roll_to(8);
			let all_events = crate::mock::System::events();
			let len = all_events.len();

			assert_eq!(
				all_events[len - 2 as usize].event,
				MetaEvent::Stake(Event::NewRound {
					block: 5,
					round: 2,
					collators: 1,
					total_balance: 20
				})
			);
			assert_ok!(Stake::set_blocks_per_round(Origin::root(), 3u32));
			roll_to(9);
			let all_events = crate::mock::System::events();
			let len = all_events.len();
			assert_eq!(
				all_events[len - 2 as usize].event,
				MetaEvent::Stake(Event::NewRound {
					block: 9,
					round: 3,
					collators: 1,
					total_balance: 20
				})
			);
		});
}

// ~~ MONETARY GOVERNANCE ~~

#[test]
fn invalid_monetary_origin_fails() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_staking_expectations(
				Origin::signed(45),
				Range { min: 3u32.into(), ideal: 4u32.into(), max: 5u32.into() }
			),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Stake::set_inflation(
				Origin::signed(45),
				Range {
					min: Perbill::from_percent(3),
					ideal: Perbill::from_percent(4),
					max: Perbill::from_percent(5)
				}
			),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Stake::set_inflation(
				Origin::signed(45),
				Range {
					min: Perbill::from_percent(3),
					ideal: Perbill::from_percent(4),
					max: Perbill::from_percent(5)
				}
			),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Stake::set_parachain_bond_account(Origin::signed(45), 11),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Stake::set_parachain_bond_reserve_percent(Origin::signed(45), Percent::from_percent(2)),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

// SET STAKING EXPECTATIONS

#[test]
fn set_staking_event_emits_event_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		// valid call succeeds
		assert_ok!(Stake::set_staking_expectations(
			Origin::root(),
			Range { min: 3u128, ideal: 4u128, max: 5u128 }
		));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::StakeExpectationsSet { min: 3u128, ideal: 4u128, max: 5u128 })
		);
	});
}

#[test]
fn set_staking_updates_storage_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::inflation_config().expect, Range { min: 700, ideal: 700, max: 700 });
		assert_ok!(Stake::set_staking_expectations(
			Origin::root(),
			Range { min: 3u128, ideal: 4u128, max: 5u128 }
		));
		assert_eq!(
			Stake::inflation_config().expect,
			Range { min: 3u128, ideal: 4u128, max: 5u128 }
		);
	});
}

#[test]
fn cannot_set_invalid_staking_expectations() {
	ExtBuilder::default().build().execute_with(|| {
		// invalid call fails
		assert_noop!(
			Stake::set_staking_expectations(
				Origin::root(),
				Range { min: 5u128, ideal: 4u128, max: 3u128 }
			),
			Error::<Test>::InvalidSchedule
		);
	});
}

#[test]
fn cannot_set_same_staking_expectations() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_staking_expectations(
			Origin::root(),
			Range { min: 3u128, ideal: 4u128, max: 5u128 }
		));
		assert_noop!(
			Stake::set_staking_expectations(
				Origin::root(),
				Range { min: 3u128, ideal: 4u128, max: 5u128 }
			),
			Error::<Test>::NoWritingSameValue
		);
	});
}

// SET INFLATION

#[test]
fn set_inflation_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		let (min, ideal, max): (Perbill, Perbill, Perbill) =
			(Perbill::from_percent(3), Perbill::from_percent(4), Perbill::from_percent(5));
		assert_ok!(Stake::set_inflation(Origin::root(), Range { min, ideal, max }));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::InflationSet {
				old_min: min,
				old_ideal: ideal,
				old_max: max,
				new_min: Perbill::from_parts(57),
				new_ideal: Perbill::from_parts(75),
				new_max: Perbill::from_parts(93)
			})
		);
	});
}

#[test]
fn set_inflation_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		let (min, ideal, max): (Perbill, Perbill, Perbill) =
			(Perbill::from_percent(3), Perbill::from_percent(4), Perbill::from_percent(5));
		assert_eq!(
			Stake::inflation_config().annual,
			Range {
				min: Perbill::from_percent(50),
				ideal: Perbill::from_percent(50),
				max: Perbill::from_percent(50)
			}
		);
		assert_eq!(
			Stake::inflation_config().round,
			Range {
				min: Perbill::from_percent(5),
				ideal: Perbill::from_percent(5),
				max: Perbill::from_percent(5)
			}
		);
		assert_ok!(Stake::set_inflation(Origin::root(), Range { min, ideal, max }),);
		assert_eq!(Stake::inflation_config().annual, Range { min, ideal, max });
		assert_eq!(
			Stake::inflation_config().round,
			Range {
				min: Perbill::from_parts(57),
				ideal: Perbill::from_parts(75),
				max: Perbill::from_parts(93)
			}
		);
	});
}

#[test]
fn cannot_set_invalid_inflation() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_inflation(
				Origin::root(),
				Range {
					min: Perbill::from_percent(5),
					ideal: Perbill::from_percent(4),
					max: Perbill::from_percent(3)
				}
			),
			Error::<Test>::InvalidSchedule
		);
	});
}

#[test]
fn cannot_set_same_inflation() {
	ExtBuilder::default().build().execute_with(|| {
		let (min, ideal, max): (Perbill, Perbill, Perbill) =
			(Perbill::from_percent(3), Perbill::from_percent(4), Perbill::from_percent(5));
		assert_ok!(Stake::set_inflation(Origin::root(), Range { min, ideal, max }),);
		assert_noop!(
			Stake::set_inflation(Origin::root(), Range { min, ideal, max }),
			Error::<Test>::NoWritingSameValue
		);
	});
}

// SET PARACHAIN BOND ACCOUNT

#[test]
fn set_parachain_bond_account_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_parachain_bond_account(Origin::root(), 11));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::ParachainBondAccountSet {
				old: Pallet::<Test>::account_id(),
				new: 11
			})
		);
	});
}

#[test]
fn set_parachain_bond_account_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::parachain_bond_info().unwrap(), Pallet::<Test>::default_bond_config());
		assert_ok!(Stake::set_parachain_bond_account(Origin::root(), 11));
		assert_eq!(Stake::parachain_bond_info().unwrap().account, 11);
	});
}

// SET PARACHAIN BOND RESERVE PERCENT

#[test]
fn set_parachain_bond_reserve_percent_event_emits_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_ok!(Stake::set_parachain_bond_reserve_percent(
			Origin::root(),
			Percent::from_percent(50)
		));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::ParachainBondReservePercentSet {
				old: Percent::from_percent(30),
				new: Percent::from_percent(50)
			})
		);
	});
}

#[test]
fn set_parachain_bond_reserve_percent_storage_updates_correctly() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Stake::parachain_bond_info().unwrap().percent, Percent::from_percent(30));
		assert_ok!(Stake::set_parachain_bond_reserve_percent(
			Origin::root(),
			Percent::from_percent(50)
		));
		assert_eq!(Stake::parachain_bond_info().unwrap().percent, Percent::from_percent(50));
	});
}

#[test]
fn cannot_set_same_parachain_bond_reserve_percent() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::set_parachain_bond_reserve_percent(Origin::root(), Percent::from_percent(30)),
			Error::<Test>::NoWritingSameValue
		);
	});
}

// ~~ PUBLIC ~~

// JOIN CANDIDATES

#[test]
fn join_candidates_event_emits_correctly() {
	ExtBuilder::default().with_balances(vec![(1, 10)]).build().execute_with(|| {
		assert_ok!(Stake::join_candidates(Origin::signed(1), 10u128, 0u32));
		assert_eq!(
			last_event(),
			MetaEvent::Stake(Event::JoinedCollatorCandidates {
				who: 1,
				amount_locked: 10u128,
				total_locked: 10u128
			})
		);
	});
}

#[test]
fn join_candidates_reserves_balance() {
	ExtBuilder::default().with_balances(vec![(1, 10)]).build().execute_with(|| {
		assert_eq!(Balances::reserved_balance(&1), 0);
		assert_eq!(Balances::free_balance(&1), 10);
		assert_ok!(Stake::join_candidates(Origin::signed(1), 10u128, 0u32));
		assert_eq!(Balances::reserved_balance(&1), 10);
		assert_eq!(Balances::free_balance(&1), 0);
	});
}

#[test]
fn join_candidates_increases_total_staked() {
	ExtBuilder::default().with_balances(vec![(1, 10)]).build().execute_with(|| {
		assert_eq!(Stake::total(), 0);
		assert_ok!(Stake::join_candidates(Origin::signed(1), 10u128, 0u32));
		assert_eq!(Stake::total(), 10);
	});
}

#[test]
fn join_candidates_creates_candidate_state() {
	ExtBuilder::default().with_balances(vec![(1, 10)]).build().execute_with(|| {
		assert!(Stake::collator_state2(1).is_none());
		assert_ok!(Stake::join_candidates(Origin::signed(1), 10u128, 0u32));
		let candidate_state = Stake::collator_state2(1).expect("just joined => exists");
		assert_eq!(candidate_state.bond, 10u128);
	});
}

#[test]
fn join_candidates_adds_to_candidate_pool() {
	ExtBuilder::default().with_balances(vec![(1, 10)]).build().execute_with(|| {
		assert!(Stake::candidate_pool().0.is_empty());
		assert_ok!(Stake::join_candidates(Origin::signed(1), 10u128, 0u32));
		let candidate_pool = Stake::candidate_pool();
		assert_eq!(candidate_pool.0[0], Bond { owner: 1, amount: 10u128 });
	});
}

#[test]
fn cannot_join_candidates_if_candidate() {
	ExtBuilder::default()
		.with_balances(vec![(1, 1000)])
		.with_candidates(vec![(1, 500)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::join_candidates(Origin::signed(1), 11u128, 100u32),
				Error::<Test>::CandidateExists
			);
		});
}

#[test]
fn cannot_join_candidates_if_nominator() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50), (2, 20)])
		.with_candidates(vec![(1, 50)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::join_candidates(Origin::signed(2), 10u128, 1u32),
				Error::<Test>::NominatorExists
			);
		});
}

#[test]
fn cannot_join_candidates_without_min_bond() {
	ExtBuilder::default().with_balances(vec![(1, 1000)]).build().execute_with(|| {
		assert_noop!(
			Stake::join_candidates(Origin::signed(1), 9u128, 100u32),
			Error::<Test>::ValBondBelowMin
		);
	});
}

#[test]
fn cannot_join_candidates_with_more_than_available_balance() {
	ExtBuilder::default().with_balances(vec![(1, 500)]).build().execute_with(|| {
		assert_noop!(
			Stake::join_candidates(Origin::signed(1), 501u128, 100u32),
			pallet_balances::Error::<Test>::InsufficientBalance
		);
	});
}

#[test]
fn insufficient_join_candidates_weight_hint_fails() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.build()
		.execute_with(|| {
			for i in 0..5 {
				assert_noop!(
					Stake::join_candidates(Origin::signed(6), 20, i),
					Error::<Test>::TooLowCandidateCountWeightHintJoinCandidates
				);
			}
		});
}

#[test]
fn sufficient_join_candidates_weight_hint_succeeds() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 20),
			(2, 20),
			(3, 20),
			(4, 20),
			(5, 20),
			(6, 20),
			(7, 20),
			(8, 20),
			(9, 20),
		])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.build()
		.execute_with(|| {
			let mut count = 5u32;
			for i in 6..10 {
				assert_ok!(Stake::join_candidates(Origin::signed(i), 20, count));
				count += 1u32;
			}
		});
}

// LEAVE CANDIDATES

#[test]
fn leave_candidates_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 1,
					collator: 1,
					scheduled_exit: 3
				})
			);
		});
}

#[test]
fn leave_candidates_unreserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Balances::reserved_balance(&1), 10);
			assert_eq!(Balances::free_balance(&1), 0);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			roll_to(30);
			assert_eq!(Balances::reserved_balance(&1), 0);
			assert_eq!(Balances::free_balance(&1), 10);
		});
}

#[test]
fn leave_candidates_decreases_total_staked() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Stake::total(), 10);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			roll_to(30);
			assert_eq!(Stake::total(), 0);
		});
}

#[test]
fn leave_candidates_removes_candidate_from_candidate_pool() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::candidate_pool().0.len(), 1);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			assert!(Stake::candidate_pool().0.is_empty());
		});
}

#[test]
fn leave_candidates_removes_candidate_state_after_exit() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			// candidate state is not immediately removed
			let candidate_state = Stake::collator_state2(1).expect("just left => still exists");
			assert_eq!(candidate_state.bond, 10u128);
			roll_to(30);
			assert!(Stake::collator_state2(1).is_none());
		});
}

#[test]
fn cannot_leave_candidates_if_not_candidate() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(Stake::leave_candidates(Origin::signed(1), 1u32), Error::<Test>::CandidateDNE);
	});
}

#[test]
fn cannot_leave_candidates_if_already_leaving_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 10)])
		.with_candidates(vec![(1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1u32));
			assert_noop!(
				Stake::leave_candidates(Origin::signed(1), 1u32),
				Error::<Test>::CandidateAlreadyLeaving
			);
		});
}

#[test]
fn insufficient_leave_candidates_weight_hint_fails() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.build()
		.execute_with(|| {
			for i in 1..6 {
				assert_noop!(
					Stake::leave_candidates(Origin::signed(i), 4u32),
					Error::<Test>::TooLowCollatorCandidateCountToLeaveCandidates
				);
			}
		});
}

#[test]
fn sufficient_leave_candidates_weight_hint_succeeds() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20)])
		.build()
		.execute_with(|| {
			let mut count = 5u32;
			for i in 1..6 {
				assert_ok!(Stake::leave_candidates(Origin::signed(i), count));
				count -= 1u32;
			}
		});
}

// GO OFFLINE

#[test]
fn go_offline_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorWentOffline { round: 1, collator: 1 })
			);
		});
}

#[test]
fn go_offline_removes_candidate_from_candidate_pool() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::candidate_pool().0.len(), 1);
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			assert!(Stake::candidate_pool().0.is_empty());
		});
}

#[test]
fn go_offline_updates_candidate_state_to_idle() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			let candidate_state = Stake::collator_state2(1).expect("is active candidate");
			assert_eq!(candidate_state.state, CollatorStatus::Active);
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			let candidate_state = Stake::collator_state2(1).expect("is candidate, just offline");
			assert_eq!(candidate_state.state, CollatorStatus::Idle);
		});
}

#[test]
fn cannot_go_offline_if_not_candidate() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(Stake::go_offline(Origin::signed(3)), Error::<Test>::CandidateDNE);
	});
}

#[test]
fn cannot_go_offline_if_already_offline() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			assert_noop!(Stake::go_offline(Origin::signed(1)), Error::<Test>::AlreadyOffline);
		});
}

// GO ONLINE

#[test]
fn go_online_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			assert_ok!(Stake::go_online(Origin::signed(1)));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorBackOnline { round: 1, collator: 1 })
			);
		});
}

#[test]
fn go_online_adds_to_candidate_pool() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			assert!(Stake::candidate_pool().0.is_empty());
			assert_ok!(Stake::go_online(Origin::signed(1)));
			assert_eq!(Stake::candidate_pool().0[0], Bond { owner: 1, amount: 20 });
		});
}

#[test]
fn go_online_storage_updates_candidate_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::go_offline(Origin::signed(1)));
			let candidate_state = Stake::collator_state2(1).expect("offline still exists");
			assert_eq!(candidate_state.state, CollatorStatus::Idle);
			assert_ok!(Stake::go_online(Origin::signed(1)));
			let candidate_state = Stake::collator_state2(1).expect("online so exists");
			assert_eq!(candidate_state.state, CollatorStatus::Active);
		});
}

#[test]
fn cannot_go_online_if_not_candidate() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(Stake::go_online(Origin::signed(3)), Error::<Test>::CandidateDNE);
	});
}

#[test]
fn cannot_go_online_if_already_online() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_noop!(Stake::go_online(Origin::signed(1)), Error::<Test>::AlreadyActive);
		});
}

// CANDIDATE BOND MORE

#[test]
fn candidate_bond_more_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::candidate_bond_more(Origin::signed(1), 30));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorBondedMore {
					collator: 1,
					old_bond: 20,
					new_bond: 50
				})
			);
		});
}

#[test]
fn candidate_bond_more_reserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::reserved_balance(&1), 20);
			assert_eq!(Balances::free_balance(&1), 30);
			assert_ok!(Stake::candidate_bond_more(Origin::signed(1), 30));
			assert_eq!(Balances::reserved_balance(&1), 50);
			assert_eq!(Balances::free_balance(&1), 0);
		});
}

#[test]
fn candidate_bond_more_increases_total() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			let mut total = Stake::total();
			assert_ok!(Stake::candidate_bond_more(Origin::signed(1), 30));
			total += 30;
			assert_eq!(Stake::total(), total);
		});
}

#[test]
fn candidate_bond_more_updates_candidate_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			let candidate_state = Stake::collator_state2(1).expect("updated => exists");
			assert_eq!(candidate_state.bond, 20);
			assert_ok!(Stake::candidate_bond_more(Origin::signed(1), 30));
			let candidate_state = Stake::collator_state2(1).expect("updated => exists");
			assert_eq!(candidate_state.bond, 50);
		});
}

#[test]
fn candidate_bond_more_updates_candidate_pool() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::candidate_pool().0[0], Bond { owner: 1, amount: 20 });
			assert_ok!(Stake::candidate_bond_more(Origin::signed(1), 30));
			assert_eq!(Stake::candidate_pool().0[0], Bond { owner: 1, amount: 50 });
		});
}

#[test]
fn cannot_candidate_bond_more_if_not_candidate() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::candidate_bond_more(Origin::signed(6), 50),
			Error::<Test>::CandidateDNE
		);
	});
}

#[test]
fn cannot_candidate_bond_more_if_insufficient_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::candidate_bond_more(Origin::signed(1), 1),
				pallet_balances::Error::<Test>::InsufficientBalance
			);
		});
}

#[test]
fn cannot_candidate_bond_more_if_leaving_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			assert_noop!(
				Stake::candidate_bond_more(Origin::signed(1), 30),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_candidate_bond_more_if_exited_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 50)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			roll_to(4);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			roll_to(30);
			assert_noop!(
				Stake::candidate_bond_more(Origin::signed(1), 30),
				Error::<Test>::CandidateDNE
			);
		});
}

// CANDIDATE BOND LESS

#[test]
fn candidate_bond_less_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::candidate_bond_less(Origin::signed(1), 10));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorBondedLess {
					collator: 1,
					old_bond: 30,
					new_bond: 20
				})
			);
		});
}

#[test]
fn candidate_bond_less_unreserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::reserved_balance(&1), 30);
			assert_eq!(Balances::free_balance(&1), 0);
			assert_ok!(Stake::candidate_bond_less(Origin::signed(1), 10));
			assert_eq!(Balances::reserved_balance(&1), 20);
			assert_eq!(Balances::free_balance(&1), 10);
		});
}

#[test]
fn candidate_bond_less_decreases_total() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			let mut total = Stake::total();
			assert_ok!(Stake::candidate_bond_less(Origin::signed(1), 10));
			total -= 10;
			assert_eq!(Stake::total(), total);
		});
}

#[test]
fn candidate_bond_less_updates_candidate_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			let candidate_state = Stake::collator_state2(1).expect("updated => exists");
			assert_eq!(candidate_state.bond, 30);
			assert_ok!(Stake::candidate_bond_less(Origin::signed(1), 10));
			let candidate_state = Stake::collator_state2(1).expect("updated => exists");
			assert_eq!(candidate_state.bond, 20);
		});
}

#[test]
fn candidate_bond_less_updates_candidate_pool() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::candidate_pool().0[0], Bond { owner: 1, amount: 30 });
			assert_ok!(Stake::candidate_bond_less(Origin::signed(1), 10));
			assert_eq!(Stake::candidate_pool().0[0], Bond { owner: 1, amount: 20 });
		});
}

#[test]
fn cannot_candidate_bond_less_if_not_candidate() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::candidate_bond_less(Origin::signed(6), 50),
			Error::<Test>::CandidateDNE
		);
	});
}

#[test]
fn cannot_candidate_bond_less_if_new_total_below_min_candidate_stk() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::candidate_bond_less(Origin::signed(1), 21),
				Error::<Test>::ValBondBelowMin
			);
		});
}

#[test]
fn cannot_candidate_bond_less_if_leaving_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			assert_noop!(
				Stake::candidate_bond_less(Origin::signed(1), 10),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_candidate_bond_less_if_exited_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			roll_to(4);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			roll_to(30);
			assert_noop!(
				Stake::candidate_bond_less(Origin::signed(1), 10),
				Error::<Test>::CandidateDNE
			);
		});
}

// NOMINATE

#[test]
fn nominate_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 10, 0, 0));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::Nomination {
					nominator: 2,
					locked: 10,
					collator: 1,
					nominator_position: NominatorAdded::AddedToTop { new_total: 40 }
				}),
			);
		});
}

#[test]
fn nominate_reserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::reserved_balance(&2), 0);
			assert_eq!(Balances::free_balance(&2), 10);
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 10, 0, 0));
			assert_eq!(Balances::reserved_balance(&2), 10);
			assert_eq!(Balances::free_balance(&2), 0);
		});
}

#[test]
fn nominate_updates_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert!(Stake::nominator_state2(2).is_none());
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 10, 0, 0));
			let nominator_state = Stake::nominator_state2(2).expect("just nominated => exists");
			assert_eq!(nominator_state.total, 10);
			assert_eq!(nominator_state.nominations.0[0], Bond { owner: 1, amount: 10 });
		});
}

#[test]
fn nominate_updates_collator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			let candidate_state = Stake::collator_state2(1).expect("registered in genesis");
			assert_eq!(candidate_state.total_backing, 30);
			assert_eq!(candidate_state.total_counted, 30);
			assert!(candidate_state.top_nominators.is_empty());
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 10, 0, 0));
			let candidate_state = Stake::collator_state2(1).expect("just nominated => exists");
			assert_eq!(candidate_state.total_backing, 40);
			assert_eq!(candidate_state.total_counted, 40);
			assert_eq!(candidate_state.top_nominators[0], Bond { owner: 2, amount: 10 });
		});
}

#[test]
fn can_nominate_immediately_after_other_join_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::join_candidates(Origin::signed(1), 20, 0));
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 20, 0, 0));
		});
}

#[test]
fn can_nominate_if_revoking() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 30), (3, 20), (4, 20)])
		.with_candidates(vec![(1, 20), (3, 20), (4, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_ok!(Stake::nominate(Origin::signed(2), 4, 10, 0, 2));
		});
}

#[test]
fn cannot_nominate_if_leaving() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			assert_noop!(
				Stake::nominate(Origin::signed(2), 1, 10, 0, 0),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_nominate_if_candidate() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20)])
		.with_candidates(vec![(1, 20), (2, 20)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominate(Origin::signed(2), 1, 10, 0, 0),
				Error::<Test>::CandidateExists
			);
		});
}

#[test]
fn cannot_nominate_if_already_nominated() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 30)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(2, 1, 20)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominate(Origin::signed(2), 1, 10, 1, 1),
				Error::<Test>::AlreadyNominatedCollator
			);
		});
}

#[test]
fn cannot_nominate_more_than_max_nominations() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 50), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_candidates(vec![(1, 20), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10), (2, 4, 10), (2, 5, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominate(Origin::signed(2), 6, 10, 0, 4),
				Error::<Test>::ExceedMaxCollatorsPerNom,
			);
		});
}

#[test]
fn sufficient_nominate_weight_hint_succeeds() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 20),
			(2, 20),
			(3, 20),
			(4, 20),
			(5, 20),
			(6, 20),
			(7, 20),
			(8, 20),
			(9, 20),
			(10, 20),
		])
		.with_candidates(vec![(1, 20), (2, 20)])
		.with_nominations(vec![(3, 1, 10), (4, 1, 10), (5, 1, 10), (6, 1, 10)])
		.build()
		.execute_with(|| {
			let mut count = 4u32;
			for i in 7..11 {
				assert_ok!(Stake::nominate(Origin::signed(i), 1, 10, count, 0u32));
				count += 1u32;
			}
			let mut count = 0u32;
			for i in 3..11 {
				assert_ok!(Stake::nominate(Origin::signed(i), 2, 10, count, 1u32));
				count += 1u32;
			}
		});
}

#[test]
fn insufficient_nominate_weight_hint_fails() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 20),
			(2, 20),
			(3, 20),
			(4, 20),
			(5, 20),
			(6, 20),
			(7, 20),
			(8, 20),
			(9, 20),
			(10, 20),
		])
		.with_candidates(vec![(1, 20), (2, 20)])
		.with_nominations(vec![(3, 1, 10), (4, 1, 10), (5, 1, 10), (6, 1, 10)])
		.build()
		.execute_with(|| {
			let mut count = 3u32;
			for i in 7..11 {
				assert_noop!(
					Stake::nominate(Origin::signed(i), 1, 10, count, 0u32),
					Error::<Test>::TooLowCollatorNominationCountToNominate
				);
			}
			// to set up for next error test
			count = 4u32;
			for i in 7..11 {
				assert_ok!(Stake::nominate(Origin::signed(i), 1, 10, count, 0u32));
				count += 1u32;
			}
			count = 0u32;
			for i in 3..11 {
				assert_noop!(
					Stake::nominate(Origin::signed(i), 2, 10, count, 0u32),
					Error::<Test>::TooLowNominationCountToNominate
				);
				count += 1u32;
			}
		});
}

// LEAVE_NOMINATORS

#[test]
fn leave_nominators_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominatorExitScheduled {
					round: 1,
					nominator: 2,
					scheduled_exit: 3
				})
			);
			roll_to(10);
			assert!(events().contains(&Event::NominatorLeft { nominator: 2, unstaked: 10 }));
		});
}

#[test]
fn leave_nominators_unreserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Balances::reserved_balance(&2), 10);
			assert_eq!(Balances::free_balance(&2), 0);
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			roll_to(10);
			assert_eq!(Balances::reserved_balance(&2), 0);
			assert_eq!(Balances::free_balance(&2), 10);
		});
}

#[test]
fn leave_nominators_decreases_total_staked() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			roll_to(10);
			assert_eq!(Stake::total(), 30);
		});
}

#[test]
fn leave_nominators_removes_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert!(Stake::nominator_state2(2).is_some());
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			roll_to(10);
			assert!(Stake::nominator_state2(2).is_none());
		});
}

#[test]
fn leave_nominators_removes_nominations_from_collator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 100), (2, 20), (3, 20), (4, 20), (5, 20)])
		.with_candidates(vec![(2, 20), (3, 20), (4, 20), (5, 20)])
		.with_nominations(vec![(1, 2, 10), (1, 3, 10), (1, 4, 10), (1, 5, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			for i in 2..6 {
				let candidate_state =
					Stake::collator_state2(i).expect("initialized in ext builder");
				assert_eq!(candidate_state.top_nominators[0], Bond { owner: 1, amount: 10 });
				assert_eq!(candidate_state.nominators.0[0], 1);
				assert_eq!(candidate_state.total_backing, 30);
			}
			assert_eq!(Stake::nominator_state2(1).unwrap().nominations.0.len(), 4usize);
			assert_ok!(Stake::leave_nominators(Origin::signed(1), 10));
			roll_to(10);
			for i in 2..6 {
				let candidate_state =
					Stake::collator_state2(i).expect("initialized in ext builder");
				assert!(candidate_state.top_nominators.is_empty());
				assert!(candidate_state.nominators.0.is_empty());
				assert_eq!(candidate_state.total_backing, 20);
			}
		});
}

#[test]
fn cannot_leave_nominators_if_leaving_through_revoking_last_nomination() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 3));
			assert_noop!(
				Stake::leave_nominators(Origin::signed(2), 2),
				Error::<Test>::NominatorAlreadyLeaving
			);
		});
}

#[test]
fn cannot_leave_nominators_if_already_leaving() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			assert_noop!(
				Stake::leave_nominators(Origin::signed(2), 1),
				Error::<Test>::NominatorAlreadyLeaving
			);
		});
}

#[test]
fn cannot_leave_nominators_if_not_nominator() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::leave_nominators(Origin::signed(2), 1),
				Error::<Test>::NominatorDNE
			);
		});
}

#[test]
fn insufficient_leave_nominators_weight_hint_fails() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(3, 1, 10), (4, 1, 10), (5, 1, 10), (6, 1, 10)])
		.build()
		.execute_with(|| {
			for i in 3..7 {
				assert_noop!(
					Stake::leave_nominators(Origin::signed(i), 0u32),
					Error::<Test>::TooLowNominationCountToLeaveNominators
				);
			}
		});
}

#[test]
fn sufficient_leave_nominators_weight_hint_succeeds() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(3, 1, 10), (4, 1, 10), (5, 1, 10), (6, 1, 10)])
		.build()
		.execute_with(|| {
			for i in 3..7 {
				assert_ok!(Stake::leave_nominators(Origin::signed(i), 1u32),);
			}
		});
}

// REVOKE_NOMINATION

#[test]
fn revoke_nomination_event_emits_exit_scheduled_if_no_nominations_left() {
	// last nomination is revocation
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominatorExitScheduled {
					round: 1,
					nominator: 2,
					scheduled_exit: 3
				})
			);
			roll_to(10);
			assert!(events().contains(&Event::NominatorLeftCollator {
				nominator: 2,
				collator: 1,
				unstaked: 10,
				total_staked: 30
			}));
			assert!(events().contains(&Event::NominatorLeft { nominator: 2, unstaked: 10 }));
		});
}

#[test]
fn revoke_nomination_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationRevocationScheduled {
					round: 1,
					nominator: 2,
					collator: 1,
					scheduled_exit: 3
				})
			);
			roll_to(10);
			assert!(events().contains(&Event::NominatorLeftCollator {
				nominator: 2,
				collator: 1,
				unstaked: 10,
				total_staked: 30
			}));
		});
}

#[test]
fn revoke_nomination_unreserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Balances::reserved_balance(&2), 10);
			assert_eq!(Balances::free_balance(&2), 0);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			roll_to(10);
			assert_eq!(Balances::reserved_balance(&2), 0);
			assert_eq!(Balances::free_balance(&2), 10);
		});
}

#[test]
fn revoke_nomination_adds_revocation_to_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert!(Stake::nominator_state2(2).expect("exists").revocations.0.is_empty());
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(Stake::nominator_state2(2).expect("exists").revocations.0[0], 1);
		});
}

#[test]
fn revoke_nomination_removes_revocation_from_nominator_state_upon_execution() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			roll_to(10);
			assert!(Stake::nominator_state2(2).expect("exists").revocations.0.is_empty());
		});
}

#[test]
fn revoke_nomination_decreases_total_staked() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			roll_to(10);
			assert_eq!(Stake::total(), 30);
		});
}

#[test]
fn revoke_nomination_for_last_nomination_removes_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert!(Stake::nominator_state2(2).is_some());
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			roll_to(10);
			assert!(Stake::nominator_state2(2).is_none());
		});
}

#[test]
fn revoke_nomination_removes_nomination_from_candidate_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_eq!(Stake::collator_state2(1).expect("exists").nominators.0.len(), 1usize);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			roll_to(10);
			assert!(Stake::collator_state2(1).expect("exists").nominators.0.is_empty());
		});
}

#[test]
fn can_revoke_nomination_if_revoking_another_nomination() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 3));
		});
}

#[test]
fn cannot_revoke_if_leaving() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 2));
			assert_noop!(
				Stake::revoke_nomination(Origin::signed(2), 3),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_revoke_nomination_if_not_nominator() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(Stake::revoke_nomination(Origin::signed(2), 1), Error::<Test>::NominatorDNE);
	});
}

#[test]
fn cannot_revoke_nomination_that_dne() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::revoke_nomination(Origin::signed(2), 3),
				Error::<Test>::NominationDNE
			);
		});
}

#[test]
fn cannot_revoke_nomination_leaving_nominator_below_min_nominator_stake() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 8), (3, 20)])
		.with_candidates(vec![(1, 20), (3, 20)])
		.with_nominations(vec![(2, 1, 5), (2, 3, 3)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::revoke_nomination(Origin::signed(2), 1),
				Error::<Test>::NomBondBelowMin
			);
		});
}

#[test]
fn revoke_nomination_after_leave_candidates_executes_during_leave_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominatorExitScheduled {
					round: 1,
					nominator: 2,
					scheduled_exit: 3
				})
			);
			roll_to(10);
			assert!(!Stake::is_nominator(&2));
			assert_eq!(Balances::reserved_balance(&2), 0);
			assert_eq!(Balances::free_balance(&2), 10);
		});
}

#[test]
fn revoke_nomination_before_leave_candidates_executes_during_leave_candidates() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominatorExitScheduled {
					round: 1,
					nominator: 2,
					scheduled_exit: 3
				})
			);
			assert_ok!(Stake::leave_candidates(Origin::signed(1), 1));
			roll_to(10);
			assert!(!Stake::is_nominator(&2));
			assert_eq!(Balances::reserved_balance(&2), 0);
			assert_eq!(Balances::free_balance(&2), 10);
		});
}

#[test]
fn nominator_bond_more_after_revoke_nomination_does_not_effect_exit() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 30), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationRevocationScheduled {
					round: 1,
					nominator: 2,
					collator: 1,
					scheduled_exit: 3
				})
			);
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 1, 10),
				Error::<Test>::CannotActBecauseRevoking
			);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 3, 10));
			roll_to(10);
			assert!(Stake::is_nominator(&2));
			assert_eq!(Balances::reserved_balance(&2), 20);
			assert_eq!(Balances::free_balance(&2), 10);
		});
}

#[test]
fn nominator_bond_less_after_revoke_nomination_does_not_effect_exit() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 30), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationRevocationScheduled {
					round: 1,
					nominator: 2,
					collator: 1,
					scheduled_exit: 3
				})
			);
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 2),
				Error::<Test>::CannotActBecauseRevoking
			);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 3, 2));
			roll_to(10);
			assert!(Stake::is_nominator(&2));
			assert_eq!(Balances::reserved_balance(&2), 8);
			assert_eq!(Balances::free_balance(&2), 22);
		});
}

// NOMINATOR BOND MORE

#[test]
fn nominator_bond_more_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationIncreased {
					nominator: 2,
					collator: 1,
					amount: 5,
					in_top: true
				})
			);
		});
}

#[test]
fn nominator_bond_more_reserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::reserved_balance(&2), 10);
			assert_eq!(Balances::free_balance(&2), 5);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(Balances::reserved_balance(&2), 15);
			assert_eq!(Balances::free_balance(&2), 0);
		});
}

#[test]
fn nominator_bond_more_increases_total_staked() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(Stake::total(), 45);
		});
}

#[test]
fn nominator_bond_more_updates_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::nominator_state2(2).expect("exists").total, 10);
			assert_eq!(
				Stake::nominator_state2(2).expect("exists").nominations.0[0],
				Bond { owner: 1, amount: 10 }
			);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(Stake::nominator_state2(2).expect("exists").total, 15);
			assert_eq!(
				Stake::nominator_state2(2).expect("exists").nominations.0[0],
				Bond { owner: 1, amount: 15 }
			);
		});
}

#[test]
fn nominator_bond_more_updates_candidate_state_top_nominators() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(
				Stake::collator_state2(1).expect("exists").top_nominators[0],
				Bond { owner: 2, amount: 10 }
			);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(
				Stake::collator_state2(1).expect("exists").top_nominators[0],
				Bond { owner: 2, amount: 15 }
			);
		});
}

#[test]
fn nominator_bond_more_updates_candidate_state_bottom_nominators() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 20), (4, 20), (5, 20), (6, 20)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10), (3, 1, 20), (4, 1, 20), (5, 1, 20), (6, 1, 20)])
		.build()
		.execute_with(|| {
			assert_eq!(
				Stake::collator_state2(1).expect("exists").bottom_nominators[0],
				Bond { owner: 2, amount: 10 }
			);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationIncreased {
					nominator: 2,
					collator: 1,
					amount: 5,
					in_top: false
				})
			);
			assert_eq!(
				Stake::collator_state2(1).expect("exists").bottom_nominators[0],
				Bond { owner: 2, amount: 15 }
			);
		});
}

#[test]
fn nominator_bond_more_increases_total() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(2), 1, 5));
			assert_eq!(Stake::total(), 45);
		});
}

#[test]
fn cannot_nominator_bond_more_if_leaving() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 1, 5),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_nominator_bond_more_if_revoking() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 25), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 1, 5),
				Error::<Test>::CannotActBecauseRevoking
			);
		});
}

#[test]
fn cannot_nominator_bond_more_if_not_nominator() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::nominator_bond_more(Origin::signed(2), 1, 5),
			Error::<Test>::NominatorDNE
		);
	});
}

#[test]
fn cannot_nominator_bond_more_if_candidate_dne() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 3, 5),
				Error::<Test>::CandidateDNE
			);
		});
}

#[test]
fn cannot_nominator_bond_more_if_nomination_dne() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 3, 5),
				Error::<Test>::NominationDNE
			);
		});
}

#[test]
fn cannot_nominator_bond_more_if_insufficient_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_more(Origin::signed(2), 1, 5),
				pallet_balances::Error::<Test>::InsufficientBalance
			);
		});
}

// NOMINATOR BOND LESS

#[test]
fn nominator_bond_less_event_emits_correctly() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::NominationDecreased {
					nominator: 2,
					collator: 1,
					amount: 5,
					in_top: true
				})
			);
		});
}

#[test]
fn nominator_bond_less_unreserves_balance() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::reserved_balance(&2), 10);
			assert_eq!(Balances::free_balance(&2), 0);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(Balances::reserved_balance(&2), 5);
			assert_eq!(Balances::free_balance(&2), 5);
		});
}

#[test]
fn nominator_bond_less_decreases_total_staked() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(Stake::total(), 35);
		});
}

#[test]
fn nominator_bond_less_updates_nominator_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::nominator_state2(2).expect("exists").total, 10);
			assert_eq!(
				Stake::nominator_state2(2).expect("exists").nominations.0[0],
				Bond { owner: 1, amount: 10 }
			);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(Stake::nominator_state2(2).expect("exists").total, 5);
			assert_eq!(
				Stake::nominator_state2(2).expect("exists").nominations.0[0],
				Bond { owner: 1, amount: 5 }
			);
		});
}

#[test]
fn nominator_bond_less_updates_candidate_state() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(
				Stake::collator_state2(1).expect("exists").top_nominators[0],
				Bond { owner: 2, amount: 10 }
			);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(
				Stake::collator_state2(1).expect("exists").top_nominators[0],
				Bond { owner: 2, amount: 5 }
			);
		});
}

#[test]
fn nominator_bond_less_decreases_total() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Stake::total(), 40);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 5));
			assert_eq!(Stake::total(), 35);
		});
}

#[test]
fn cannot_nominator_bond_less_if_leaving() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 15)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::leave_nominators(Origin::signed(2), 1));
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 1),
				Error::<Test>::CannotActBecauseLeaving
			);
		});
}

#[test]
fn cannot_nominator_bond_less_if_revoking() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 25), (3, 20)])
		.with_candidates(vec![(1, 30), (3, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 1),
				Error::<Test>::CannotActBecauseRevoking
			);
		});
}

#[test]
fn cannot_nominator_bond_less_if_not_nominator() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			Stake::nominator_bond_less(Origin::signed(2), 1, 5),
			Error::<Test>::NominatorDNE
		);
	});
}

#[test]
fn cannot_nominator_bond_less_if_candidate_dne() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 3, 5),
				Error::<Test>::CandidateDNE
			);
		});
}

#[test]
fn cannot_nominator_bond_less_if_nomination_dne() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 3, 5),
				Error::<Test>::NominationDNE
			);
		});
}

#[test]
fn cannot_nominator_bond_less_below_min_collator_stk() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 6),
				Error::<Test>::NomBondBelowMin
			);
		});
}

#[test]
fn cannot_nominator_bond_less_more_than_total_nomination() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 10)])
		.with_candidates(vec![(1, 30)])
		.with_nominations(vec![(2, 1, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 11),
				Error::<Test>::NomBondBelowMin
			);
		});
}

#[test]
fn cannot_nominator_bond_less_below_min_nomination() {
	ExtBuilder::default()
		.with_balances(vec![(1, 30), (2, 20), (3, 30)])
		.with_candidates(vec![(1, 30), (3, 30)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10)])
		.build()
		.execute_with(|| {
			assert_noop!(
				Stake::nominator_bond_less(Origin::signed(2), 1, 8),
				Error::<Test>::NominationBelowMin
			);
		});
}

#[test]
fn nominator_bond_less_updates_just_bottom_nominations() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 10), (3, 11), (4, 12), (5, 14), (6, 15)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(2, 1, 10), (3, 1, 11), (4, 1, 12), (5, 1, 14), (6, 1, 15)])
		.build()
		.execute_with(|| {
			let pre_call_collator_state =
				Stake::collator_state2(&1).expect("nominated by all so exists");
			assert_ok!(Stake::nominator_bond_less(Origin::signed(2), 1, 2));
			let post_call_collator_state =
				Stake::collator_state2(&1).expect("nominated by all so exists");
			let mut not_equal = false;
			for Bond { owner, amount } in pre_call_collator_state.bottom_nominators {
				for Bond { owner: post_owner, amount: post_amount } in
					&post_call_collator_state.bottom_nominators
				{
					if &owner == post_owner {
						if &amount != post_amount {
							not_equal = true;
							break
						}
					}
				}
			}
			assert!(not_equal);
			let mut equal = true;
			for Bond { owner, amount } in pre_call_collator_state.top_nominators {
				for Bond { owner: post_owner, amount: post_amount } in
					&post_call_collator_state.top_nominators
				{
					if &owner == post_owner {
						if &amount != post_amount {
							equal = false;
							break
						}
					}
				}
			}
			assert!(equal);
			assert_eq!(
				pre_call_collator_state.total_backing - 2,
				post_call_collator_state.total_backing
			);
			assert_eq!(
				pre_call_collator_state.total_counted,
				post_call_collator_state.total_counted
			);
		});
}

#[test]
fn nominator_bond_less_does_not_delete_bottom_nominations() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 10), (3, 11), (4, 12), (5, 14), (6, 15)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(2, 1, 10), (3, 1, 11), (4, 1, 12), (5, 1, 14), (6, 1, 15)])
		.build()
		.execute_with(|| {
			let pre_call_collator_state =
				Stake::collator_state2(&1).expect("nominated by all so exists");
			assert_ok!(Stake::nominator_bond_less(Origin::signed(6), 1, 4));
			let post_call_collator_state =
				Stake::collator_state2(&1).expect("nominated by all so exists");
			let mut equal = true;
			for Bond { owner, amount } in pre_call_collator_state.bottom_nominators {
				for Bond { owner: post_owner, amount: post_amount } in
					&post_call_collator_state.bottom_nominators
				{
					if &owner == post_owner {
						if &amount != post_amount {
							equal = false;
							break
						}
					}
				}
			}
			assert!(equal);
			let mut not_equal = false;
			for Bond { owner, amount } in pre_call_collator_state.top_nominators {
				for Bond { owner: post_owner, amount: post_amount } in
					&post_call_collator_state.top_nominators
				{
					if &owner == post_owner {
						if &amount != post_amount {
							not_equal = true;
							break
						}
					}
				}
			}
			assert!(not_equal);
			assert_eq!(
				pre_call_collator_state.total_backing - 4,
				post_call_collator_state.total_backing
			);
			assert_eq!(
				pre_call_collator_state.total_counted - 4,
				post_call_collator_state.total_counted
			);
		});
}

// ~~ PROPERTY-BASED TESTS ~~

#[test]
fn nominator_schedule_revocation_total() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 40), (3, 20), (4, 20), (5, 20)])
		.with_candidates(vec![(1, 20), (3, 20), (4, 20), (5, 20)])
		.with_nominations(vec![(2, 1, 10), (2, 3, 10), (2, 4, 10)])
		.build()
		.execute_with(|| {
			roll_to(1);
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 1));
			assert_eq!(Stake::nominator_state2(2).expect("exists").scheduled_revocations_total, 10);
			roll_to(10);
			assert_eq!(Stake::nominator_state2(2).expect("exists").scheduled_revocations_total, 0);
			assert_ok!(Stake::nominate(Origin::signed(2), 5, 10, 0, 2));
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 3));
			assert_ok!(Stake::revoke_nomination(Origin::signed(2), 4));
			assert_eq!(Stake::nominator_state2(2).expect("exists").scheduled_revocations_total, 20);
			roll_to(20);
			assert_eq!(Stake::nominator_state2(2).expect("exists").scheduled_revocations_total, 0);
		});
}

#[test]
fn parachain_bond_inflation_reserve_matches_config() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 100),
			(2, 100),
			(3, 100),
			(4, 100),
			(5, 100),
			(6, 100),
			(7, 100),
			(8, 100),
			(9, 100),
			(10, 100),
			(11, 1),
		])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 10)])
		.with_nominations(vec![(6, 1, 10), (7, 1, 10), (8, 2, 10), (9, 2, 10), (10, 1, 10)])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::free_balance(&11), 1);
			// set parachain bond account so DefaultParachainBondReservePercent = 30% of
			// inflation is allocated to this account hereafter
			assert_ok!(Stake::set_parachain_bond_account(Origin::root(), 11));
			roll_to(8);
			// chooses top TotalSelectedCandidates (5), in order
			let mut expected = vec![
				Event::ParachainBondAccountSet { old: Pallet::<Test>::account_id(), new: 11 },
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 140 },
			];
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 1);
			// ~ set block author as 1 for all blocks this round
			set_author(2, 1, 100);
			roll_to(16);
			// distribute total issuance to collator 1 and its nominators 6, 7, 19
			let mut new = vec![
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 140 },
				Event::ReservedForParachainBond { who: 11, bond: 15 },
				Event::Rewarded { who: 1, amount: 20 },
				Event::Rewarded { who: 6, amount: 5 },
				Event::Rewarded { who: 7, amount: 5 },
				Event::Rewarded { who: 10, amount: 5 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 4, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 15, round: 4, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 16);
			// ~ set block author as 1 for all blocks this round
			set_author(3, 1, 100);
			set_author(4, 1, 100);
			set_author(5, 1, 100);
			// 1. ensure nominators are paid for 2 rounds after they leave
			assert_noop!(
				Stake::leave_nominators(Origin::signed(66), 10),
				Error::<Test>::NominatorDNE
			);
			assert_ok!(Stake::leave_nominators(Origin::signed(6), 10));
			// fast forward to block in which nominator 6 exit executes
			roll_to(25);
			let mut new2 = vec![
				Event::NominatorExitScheduled { round: 4, nominator: 6, scheduled_exit: 6 },
				Event::ReservedForParachainBond { who: 11, bond: 16 },
				Event::Rewarded { who: 1, amount: 21 },
				Event::Rewarded { who: 6, amount: 5 },
				Event::Rewarded { who: 7, amount: 5 },
				Event::Rewarded { who: 10, amount: 5 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 5, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 20, round: 5, collators: 5, total_balance: 140 },
				Event::ReservedForParachainBond { who: 11, bond: 16 },
				Event::Rewarded { who: 1, amount: 22 },
				Event::Rewarded { who: 6, amount: 6 },
				Event::Rewarded { who: 7, amount: 6 },
				Event::Rewarded { who: 10, amount: 6 },
				Event::NominatorLeftCollator {
					nominator: 6,
					collator: 1,
					unstaked: 10,
					total_staked: 40,
				},
				Event::NominatorLeft { nominator: 6, unstaked: 10 },
				Event::CollatorChosen { round: 6, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 6, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 6, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 6, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 6, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 25, round: 6, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new2);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 48);
			assert_ok!(Stake::set_parachain_bond_reserve_percent(
				Origin::root(),
				Percent::from_percent(50)
			));
			// 6 won't be paid for this round because they left already
			set_author(6, 1, 100);
			roll_to(30);
			// keep paying 6
			let mut new3 = vec![
				Event::ParachainBondReservePercentSet {
					old: Percent::from_percent(30),
					new: Percent::from_percent(50),
				},
				Event::ReservedForParachainBond { who: 11, bond: 29 },
				Event::Rewarded { who: 1, amount: 19 },
				Event::Rewarded { who: 6, amount: 3 },
				Event::Rewarded { who: 7, amount: 3 },
				Event::Rewarded { who: 10, amount: 3 },
				Event::CollatorChosen { round: 7, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 7, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 7, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 7, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 7, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 30, round: 7, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new3);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 77);
			set_author(7, 1, 100);
			roll_to(35);
			// no more paying 6
			let mut new4 = vec![
				Event::ReservedForParachainBond { who: 11, bond: 30 },
				Event::Rewarded { who: 1, amount: 21 },
				Event::Rewarded { who: 7, amount: 5 },
				Event::Rewarded { who: 10, amount: 5 },
				Event::CollatorChosen { round: 8, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 8, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 8, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 8, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 8, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 35, round: 8, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new4);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 107);
			set_author(8, 1, 100);
			assert_ok!(Stake::nominate(Origin::signed(8), 1, 10, 10, 10));
			roll_to(40);
			// new nomination is not rewarded yet
			let mut new5 = vec![
				Event::Nomination {
					nominator: 8,
					locked: 10,
					collator: 1,
					nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
				},
				Event::ReservedForParachainBond { who: 11, bond: 32 },
				Event::Rewarded { who: 1, amount: 22 },
				Event::Rewarded { who: 7, amount: 5 },
				Event::Rewarded { who: 10, amount: 5 },
				Event::CollatorChosen { round: 9, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 9, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 9, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 9, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 9, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 40, round: 9, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new5);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 139);
			set_author(9, 1, 100);
			roll_to(45);
			// new nomination is still not rewarded yet
			let mut new6 = vec![
				Event::ReservedForParachainBond { who: 11, bond: 33 },
				Event::Rewarded { who: 1, amount: 23 },
				Event::Rewarded { who: 7, amount: 5 },
				Event::Rewarded { who: 10, amount: 5 },
				Event::CollatorChosen { round: 10, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 10, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 10, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 10, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 10, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 45, round: 10, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new6);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 172);
			roll_to(50);
			// new nomination is rewarded, 2 rounds after joining (`RewardPaymentDelay` is
			// 2)
			let mut new7 = vec![
				Event::ReservedForParachainBond { who: 11, bond: 35 },
				Event::Rewarded { who: 1, amount: 22 },
				Event::Rewarded { who: 7, amount: 4 },
				Event::Rewarded { who: 8, amount: 4 },
				Event::Rewarded { who: 10, amount: 4 },
				Event::CollatorChosen { round: 11, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 11, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 11, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 11, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 11, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 50, round: 11, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new7);
			assert_eq!(events(), expected);
			assert_eq!(Balances::free_balance(&11), 207);
		});
}

#[test]
fn paid_collator_commission_matches_config() {
	ExtBuilder::default()
		.with_balances(vec![(1, 100), (2, 100), (3, 100), (4, 100), (5, 100), (6, 100)])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![(2, 1, 10), (3, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// chooses top TotalSelectedCandidates (5), in order
			let mut expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 40 },
				Event::NewRound { block: 5, round: 2, collators: 1, total_balance: 40 },
			];
			assert_eq!(events(), expected);
			assert_ok!(Stake::join_candidates(Origin::signed(4), 20u128, 100u32));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::JoinedCollatorCandidates {
					who: 4,
					amount_locked: 20u128,
					total_locked: 60u128
				})
			);
			roll_to(9);
			assert_ok!(Stake::nominate(Origin::signed(5), 4, 10, 10, 10));
			assert_ok!(Stake::nominate(Origin::signed(6), 4, 10, 10, 10));
			roll_to(11);
			let mut new = vec![
				Event::JoinedCollatorCandidates { who: 4, amount_locked: 20, total_locked: 60 },
				Event::Nomination {
					nominator: 5,
					locked: 10,
					collator: 4,
					nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
				},
				Event::Nomination {
					nominator: 6,
					locked: 10,
					collator: 4,
					nominator_position: NominatorAdded::AddedToTop { new_total: 40 },
				},
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 40 },
				Event::NewRound { block: 10, round: 3, collators: 2, total_balance: 80 },
			];
			expected.append(&mut new);
			assert_eq!(events(), expected);
			// only amount author with id 4
			set_author(3, 4, 100);
			roll_to(21);
			// 20% of 10 is commission + due_portion (4) = 2 + 4 = 6
			// all nominator payouts are 10-2 = 8 * stake_pct
			let mut new2 = vec![
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 40 },
				Event::NewRound { block: 15, round: 4, collators: 2, total_balance: 80 },
				Event::Rewarded { who: 4, amount: 18 },
				Event::Rewarded { who: 5, amount: 6 },
				Event::Rewarded { who: 6, amount: 6 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 40 },
				Event::NewRound { block: 20, round: 5, collators: 2, total_balance: 80 },
			];
			expected.append(&mut new2);
			assert_eq!(events(), expected);
		});
}

#[test]
fn collator_exit_executes_after_delay() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 1000),
			(2, 300),
			(3, 100),
			(4, 100),
			(5, 100),
			(6, 100),
			(7, 100),
			(8, 9),
			(9, 4),
		])
		.with_candidates(vec![(1, 500), (2, 200)])
		.with_nominations(vec![(3, 1, 100), (4, 1, 100), (5, 2, 100), (6, 2, 100)])
		.build()
		.execute_with(|| {
			roll_to(11);
			assert_ok!(Stake::leave_candidates(Origin::signed(2), 2));
			let info = Stake::collator_state2(&2).unwrap();
			assert_eq!(info.state, CollatorStatus::Leaving(5));
			roll_to(21);
			// we must exclude leaving collators from rewards while
			// holding them retroactively accountable for previous faults
			// (within the last T::SlashingWindow blocks)
			let expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 700 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 400 },
				Event::NewRound { block: 5, round: 2, collators: 2, total_balance: 1100 },
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 700 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 400 },
				Event::NewRound { block: 10, round: 3, collators: 2, total_balance: 1100 },
				Event::CollatorScheduledExit { round: 3, collator: 2, scheduled_exit: 5 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 700 },
				Event::NewRound { block: 15, round: 4, collators: 1, total_balance: 700 },
				Event::CollatorLeft { collator: 2, amount_unlocked: 400, total_locked: 700 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 700 },
				Event::NewRound { block: 20, round: 5, collators: 1, total_balance: 700 },
			];
			assert_eq!(events(), expected);
		});
}

#[test]
fn collator_selection_chooses_top_candidates() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 1000),
			(2, 1000),
			(3, 1000),
			(4, 1000),
			(5, 1000),
			(6, 1000),
			(7, 33),
			(8, 33),
			(9, 33),
		])
		.with_candidates(vec![(1, 100), (2, 90), (3, 80), (4, 70), (5, 60), (6, 50)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// should choose top TotalSelectedCandidates (5), in order
			let expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 400 },
			];
			assert_eq!(events(), expected);
			assert_ok!(Stake::leave_candidates(Origin::signed(6), 6));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 2,
					collator: 6,
					scheduled_exit: 4
				})
			);
			roll_to(21);
			assert_ok!(Stake::join_candidates(Origin::signed(6), 69u128, 100u32));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::JoinedCollatorCandidates {
					who: 6,
					amount_locked: 69u128,
					total_locked: 469u128
				})
			);
			roll_to(27);
			// should choose top TotalSelectedCandidates (5), in order
			let expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 400 },
				Event::CollatorScheduledExit { round: 2, collator: 6, scheduled_exit: 4 },
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 400 },
				Event::CollatorLeft { collator: 6, amount_unlocked: 50, total_locked: 400 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 4, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 15, round: 4, collators: 5, total_balance: 400 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 5, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 20, round: 5, collators: 5, total_balance: 400 },
				Event::JoinedCollatorCandidates { who: 6, amount_locked: 69, total_locked: 469 },
				Event::CollatorChosen { round: 6, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 6, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 6, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 6, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 6, collator: 6, total_exposed_amount: 69 },
				Event::NewRound { block: 25, round: 6, collators: 5, total_balance: 409 },
			];
			assert_eq!(events(), expected);
		});
}

#[test]
fn exit_queue_executes_in_order() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 1000),
			(2, 1000),
			(3, 1000),
			(4, 1000),
			(5, 1000),
			(6, 1000),
			(7, 33),
			(8, 33),
			(9, 33),
		])
		.with_candidates(vec![(1, 100), (2, 90), (3, 80), (4, 70), (5, 60), (6, 50)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// should choose top TotalSelectedCandidates (5), in order
			let mut expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 400 },
			];
			assert_eq!(events(), expected);
			assert_ok!(Stake::leave_candidates(Origin::signed(6), 6));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 2,
					collator: 6,
					scheduled_exit: 4
				})
			);
			roll_to(11);
			assert_ok!(Stake::leave_candidates(Origin::signed(5), 5));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 3,
					collator: 5,
					scheduled_exit: 5
				})
			);
			roll_to(16);
			assert_ok!(Stake::leave_candidates(Origin::signed(4), 4));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 4,
					collator: 4,
					scheduled_exit: 6
				})
			);
			roll_to(21);
			let mut new_events = vec![
				Event::CollatorScheduledExit { round: 2, collator: 6, scheduled_exit: 4 },
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 400 },
				Event::CollatorScheduledExit { round: 3, collator: 5, scheduled_exit: 5 },
				Event::CollatorLeft { collator: 6, amount_unlocked: 50, total_locked: 400 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 70 },
				Event::NewRound { block: 15, round: 4, collators: 4, total_balance: 340 },
				Event::CollatorScheduledExit { round: 4, collator: 4, scheduled_exit: 6 },
				Event::CollatorLeft { collator: 5, amount_unlocked: 60, total_locked: 340 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 80 },
				Event::NewRound { block: 20, round: 5, collators: 3, total_balance: 270 },
			];
			expected.append(&mut new_events);
			assert_eq!(events(), expected);
		});
}

#[test]
fn payout_distribution_to_solo_collators() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 1000),
			(2, 1000),
			(3, 1000),
			(4, 1000),
			(5, 1000),
			(6, 1000),
			(7, 33),
			(8, 33),
			(9, 33),
		])
		.with_candidates(vec![(1, 100), (2, 90), (3, 80), (4, 70), (5, 60), (6, 50)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// should choose top TotalCandidatesSelected (5), in order
			let mut expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 400 },
			];
			assert_eq!(events(), expected);
			// ~ set block author as 1 for all blocks this round
			set_author(2, 1, 100);
			roll_to(16);
			// pay total issuance to 1
			let mut new = vec![
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 400 },
				Event::Rewarded { who: 1, amount: 305 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 4, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 15, round: 4, collators: 5, total_balance: 400 },
			];
			expected.append(&mut new);
			assert_eq!(events(), expected);
			// ~ set block author as 1 for 3 blocks this round
			set_author(4, 1, 60);
			// ~ set block author as 2 for 2 blocks this round
			set_author(4, 2, 40);
			roll_to(26);
			// pay 60% total issuance to 1 and 40% total issuance to 2
			let mut new1 = vec![
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 5, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 20, round: 5, collators: 5, total_balance: 400 },
				Event::Rewarded { who: 1, amount: 192 },
				Event::Rewarded { who: 2, amount: 128 },
				Event::CollatorChosen { round: 6, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 6, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 6, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 6, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 6, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 25, round: 6, collators: 5, total_balance: 400 },
			];
			expected.append(&mut new1);
			assert_eq!(events(), expected);
			// ~ each collator produces 1 block this round
			set_author(6, 1, 20);
			set_author(6, 2, 20);
			set_author(6, 3, 20);
			set_author(6, 4, 20);
			set_author(6, 5, 20);
			roll_to(36);
			// pay 20% issuance for all collators
			let mut new2 = vec![
				Event::CollatorChosen { round: 7, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 7, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 7, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 7, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 7, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 30, round: 7, collators: 5, total_balance: 400 },
				Event::Rewarded { who: 5, amount: 67 },
				Event::Rewarded { who: 3, amount: 67 },
				Event::Rewarded { who: 4, amount: 67 },
				Event::Rewarded { who: 1, amount: 67 },
				Event::Rewarded { who: 2, amount: 67 },
				Event::CollatorChosen { round: 8, collator: 1, total_exposed_amount: 100 },
				Event::CollatorChosen { round: 8, collator: 2, total_exposed_amount: 90 },
				Event::CollatorChosen { round: 8, collator: 3, total_exposed_amount: 80 },
				Event::CollatorChosen { round: 8, collator: 4, total_exposed_amount: 70 },
				Event::CollatorChosen { round: 8, collator: 5, total_exposed_amount: 60 },
				Event::NewRound { block: 35, round: 8, collators: 5, total_balance: 400 },
			];
			expected.append(&mut new2);
			assert_eq!(events(), expected);
			// check that distributing rewards clears awarded pts
			assert!(Stake::awarded_pts(1, 1).is_zero());
			assert!(Stake::awarded_pts(4, 1).is_zero());
			assert!(Stake::awarded_pts(4, 2).is_zero());
			assert!(Stake::awarded_pts(6, 1).is_zero());
			assert!(Stake::awarded_pts(6, 2).is_zero());
			assert!(Stake::awarded_pts(6, 3).is_zero());
			assert!(Stake::awarded_pts(6, 4).is_zero());
			assert!(Stake::awarded_pts(6, 5).is_zero());
		});
}

#[test]
fn multiple_nominations() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 100),
			(2, 100),
			(3, 100),
			(4, 100),
			(5, 100),
			(6, 100),
			(7, 100),
			(8, 100),
			(9, 100),
			(10, 100),
		])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 10)])
		.with_nominations(vec![(6, 1, 10), (7, 1, 10), (8, 2, 10), (9, 2, 10), (10, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// chooses top TotalSelectedCandidates (5), in order
			let mut expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 140 },
			];
			assert_eq!(events(), expected);
			assert_ok!(Stake::nominate(Origin::signed(6), 2, 10, 10, 10));
			assert_ok!(Stake::nominate(Origin::signed(6), 3, 10, 10, 10));
			assert_ok!(Stake::nominate(Origin::signed(6), 4, 10, 10, 10));
			roll_to(16);
			let mut new = vec![
				Event::Nomination {
					nominator: 6,
					locked: 10,
					collator: 2,
					nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
				},
				Event::Nomination {
					nominator: 6,
					locked: 10,
					collator: 3,
					nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
				},
				Event::Nomination {
					nominator: 6,
					locked: 10,
					collator: 4,
					nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
				},
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 170 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 4, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 15, round: 4, collators: 5, total_balance: 170 },
			];
			expected.append(&mut new);
			assert_eq!(events(), expected);
			roll_to(21);
			assert_ok!(Stake::nominate(Origin::signed(7), 2, 80, 10, 10));
			assert_ok!(Stake::nominate(Origin::signed(10), 2, 10, 10, 10),);
			roll_to(26);
			let mut new2 = vec![
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 5, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 20, round: 5, collators: 5, total_balance: 170 },
				Event::Nomination {
					nominator: 7,
					locked: 80,
					collator: 2,
					nominator_position: NominatorAdded::AddedToTop { new_total: 130 },
				},
				Event::Nomination {
					nominator: 10,
					locked: 10,
					collator: 2,
					nominator_position: NominatorAdded::AddedToBottom,
				},
				Event::CollatorChosen { round: 6, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 6, collator: 2, total_exposed_amount: 130 },
				Event::CollatorChosen { round: 6, collator: 3, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 6, collator: 4, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 6, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 25, round: 6, collators: 5, total_balance: 250 },
			];
			expected.append(&mut new2);
			assert_eq!(events(), expected);
			assert_ok!(Stake::leave_candidates(Origin::signed(2), 5));
			assert_eq!(
				last_event(),
				MetaEvent::Stake(Event::CollatorScheduledExit {
					round: 6,
					collator: 2,
					scheduled_exit: 8
				})
			);
			roll_to(31);
			let mut new3 = vec![
				Event::CollatorScheduledExit { round: 6, collator: 2, scheduled_exit: 8 },
				Event::CollatorChosen { round: 7, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 7, collator: 3, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 7, collator: 4, total_exposed_amount: 30 },
				Event::CollatorChosen { round: 7, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 30, round: 7, collators: 4, total_balance: 120 },
			];
			expected.append(&mut new3);
			assert_eq!(events(), expected);
			// verify that nominations are removed after collator leaves, not before
			assert_eq!(Stake::nominator_state2(7).unwrap().total, 90);
			assert_eq!(Stake::nominator_state2(7).unwrap().nominations.0.len(), 2usize);
			assert_eq!(Stake::nominator_state2(6).unwrap().total, 40);
			assert_eq!(Stake::nominator_state2(6).unwrap().nominations.0.len(), 4usize);
			assert_eq!(Balances::reserved_balance(&6), 40);
			assert_eq!(Balances::reserved_balance(&7), 90);
			assert_eq!(Balances::free_balance(&6), 60);
			assert_eq!(Balances::free_balance(&7), 10);
			roll_to(40);
			assert_eq!(Stake::nominator_state2(7).unwrap().total, 10);
			assert_eq!(Stake::nominator_state2(6).unwrap().total, 30);
			assert_eq!(Stake::nominator_state2(7).unwrap().nominations.0.len(), 1usize);
			assert_eq!(Stake::nominator_state2(6).unwrap().nominations.0.len(), 3usize);
			assert_eq!(Balances::reserved_balance(&6), 30);
			assert_eq!(Balances::reserved_balance(&7), 10);
			assert_eq!(Balances::free_balance(&6), 70);
			assert_eq!(Balances::free_balance(&7), 90);
		});
}

#[test]
fn payouts_follow_nomination_changes() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 100),
			(2, 100),
			(3, 100),
			(4, 100),
			(5, 100),
			(6, 100),
			(7, 100),
			(8, 100),
			(9, 100),
			(10, 100),
		])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 10)])
		.with_nominations(vec![(6, 1, 10), (7, 1, 10), (8, 2, 10), (9, 2, 10), (10, 1, 10)])
		.build()
		.execute_with(|| {
			roll_to(8);
			// chooses top TotalSelectedCandidates (5), in order
			let mut expected = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 2, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 5, round: 2, collators: 5, total_balance: 140 },
			];
			assert_eq!(events(), expected);
			// ~ set block author as 1 for all blocks this round
			set_author(2, 1, 100);
			roll_to(16);
			// distribute total issuance to collator 1 and its nominators 6, 7, 19
			let mut new = vec![
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 3, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 10, round: 3, collators: 5, total_balance: 140 },
				Event::Rewarded { who: 1, amount: 26 },
				Event::Rewarded { who: 6, amount: 8 },
				Event::Rewarded { who: 7, amount: 8 },
				Event::Rewarded { who: 10, amount: 8 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 4, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 15, round: 4, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new);
			assert_eq!(events(), expected);
			// ~ set block author as 1 for all blocks this round
			set_author(3, 1, 100);
			set_author(4, 1, 100);
			set_author(5, 1, 100);
			// 1. ensure nominators are paid for 2 rounds after they leave
			assert_noop!(
				Stake::leave_nominators(Origin::signed(66), 10),
				Error::<Test>::NominatorDNE
			);
			assert_ok!(Stake::leave_nominators(Origin::signed(6), 10));
			// fast forward to block in which nominator 6 exit executes
			roll_to(25);
			// keep paying 6 (note: inflation is in terms of total issuance so that's why 1
			// is 21)
			let mut new2 = vec![
				Event::NominatorExitScheduled { round: 4, nominator: 6, scheduled_exit: 6 },
				Event::Rewarded { who: 1, amount: 27 },
				Event::Rewarded { who: 6, amount: 8 },
				Event::Rewarded { who: 7, amount: 8 },
				Event::Rewarded { who: 10, amount: 8 },
				Event::CollatorChosen { round: 5, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 5, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 5, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 5, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 5, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 20, round: 5, collators: 5, total_balance: 140 },
				Event::Rewarded { who: 1, amount: 29 },
				Event::Rewarded { who: 6, amount: 9 },
				Event::Rewarded { who: 7, amount: 9 },
				Event::Rewarded { who: 10, amount: 9 },
				Event::NominatorLeftCollator {
					nominator: 6,
					collator: 1,
					unstaked: 10,
					total_staked: 40,
				},
				Event::NominatorLeft { nominator: 6, unstaked: 10 },
				Event::CollatorChosen { round: 6, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 6, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 6, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 6, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 6, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 25, round: 6, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new2);
			assert_eq!(events(), expected);
			// 6 won't be paid for this round because they left already
			set_author(6, 1, 100);
			roll_to(30);
			// keep paying 6
			let mut new3 = vec![
				Event::Rewarded { who: 1, amount: 30 },
				Event::Rewarded { who: 6, amount: 9 },
				Event::Rewarded { who: 7, amount: 9 },
				Event::Rewarded { who: 10, amount: 9 },
				Event::CollatorChosen { round: 7, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 7, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 7, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 7, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 7, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 30, round: 7, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new3);
			assert_eq!(events(), expected);
			set_author(7, 1, 100);
			roll_to(35);
			// no more paying 6
			let mut new4 = vec![
				Event::Rewarded { who: 1, amount: 36 },
				Event::Rewarded { who: 7, amount: 12 },
				Event::Rewarded { who: 10, amount: 12 },
				Event::CollatorChosen { round: 8, collator: 1, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 8, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 8, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 8, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 8, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 35, round: 8, collators: 5, total_balance: 130 },
			];
			expected.append(&mut new4);
			assert_eq!(events(), expected);
			set_author(8, 1, 100);
			assert_ok!(Stake::nominate(Origin::signed(8), 1, 10, 10, 10));
			roll_to(40);
			// new nomination is not rewarded yet
			let mut new5 = vec![
				Event::Nomination {
					nominator: 8,
					locked: 10,
					collator: 1,
					nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
				},
				Event::Rewarded { who: 1, amount: 38 },
				Event::Rewarded { who: 7, amount: 13 },
				Event::Rewarded { who: 10, amount: 13 },
				Event::CollatorChosen { round: 9, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 9, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 9, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 9, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 9, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 40, round: 9, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new5);
			assert_eq!(events(), expected);
			set_author(9, 1, 100);
			roll_to(45);
			// new nomination is still not rewarded yet
			let mut new6 = vec![
				Event::Rewarded { who: 1, amount: 40 },
				Event::Rewarded { who: 7, amount: 13 },
				Event::Rewarded { who: 10, amount: 13 },
				Event::CollatorChosen { round: 10, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 10, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 10, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 10, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 10, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 45, round: 10, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new6);
			assert_eq!(events(), expected);
			roll_to(50);
			// new nomination is rewarded for first time, 2 rounds after joining
			// (`RewardPaymentDelay` = 2)
			let mut new7 = vec![
				Event::Rewarded { who: 1, amount: 36 },
				Event::Rewarded { who: 7, amount: 11 },
				Event::Rewarded { who: 8, amount: 11 },
				Event::Rewarded { who: 10, amount: 11 },
				Event::CollatorChosen { round: 11, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 11, collator: 2, total_exposed_amount: 40 },
				Event::CollatorChosen { round: 11, collator: 3, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 11, collator: 4, total_exposed_amount: 20 },
				Event::CollatorChosen { round: 11, collator: 5, total_exposed_amount: 10 },
				Event::NewRound { block: 50, round: 11, collators: 5, total_balance: 140 },
			];
			expected.append(&mut new7);
			assert_eq!(events(), expected);
		});
}

#[test]
fn nominations_merged_before_reward_payout() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 20), (3, 20), (4, 20), (5, 120)])
		.with_candidates(vec![(1, 20), (2, 20), (3, 20), (4, 20)])
		.with_nominations(vec![(5, 1, 30), (5, 2, 30), (5, 3, 30), (5, 4, 30)])
		.build()
		.execute_with(|| {
			roll_to(8);
			set_author(1, 1, 1);
			set_author(1, 2, 1);
			set_author(1, 3, 1);
			set_author(1, 4, 1);
			roll_to(16);
			let expected_events = vec![
				Event::CollatorChosen { round: 2, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 3, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 2, collator: 4, total_exposed_amount: 50 },
				Event::NewRound { block: 5, round: 2, collators: 4, total_balance: 200 },
				Event::Rewarded { who: 3, amount: 1 },
				Event::Rewarded { who: 4, amount: 1 },
				Event::Rewarded { who: 1, amount: 1 },
				Event::Rewarded { who: 2, amount: 1 },
				// ALL REWARDS FOR 5 are merged into one payment + event
				Event::Rewarded { who: 5, amount: 4 },
				Event::CollatorChosen { round: 3, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 3, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 3, collator: 4, total_exposed_amount: 50 },
				Event::NewRound { block: 10, round: 3, collators: 4, total_balance: 200 },
				Event::CollatorChosen { round: 4, collator: 1, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 2, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 3, total_exposed_amount: 50 },
				Event::CollatorChosen { round: 4, collator: 4, total_exposed_amount: 50 },
				Event::NewRound { block: 15, round: 4, collators: 4, total_balance: 200 },
			];
			assert_eq!(events(), expected_events);
		});
}

#[test]
// MaxNominatorsPerCollator = 4
fn bottom_nominations_are_empty_when_top_nominations_not_full() {
	ExtBuilder::default()
		.with_balances(vec![(1, 20), (2, 10), (3, 10), (4, 10), (5, 10)])
		.with_candidates(vec![(1, 20)])
		.build()
		.execute_with(|| {
			// no top nominators => no bottom nominators
			let collator_state = Stake::collator_state2(1).unwrap();
			assert!(collator_state.top_nominators.is_empty());
			assert!(collator_state.bottom_nominators.is_empty());
			// 1 nominator => 1 top nominator, 0 bottom nominators
			assert_ok!(Stake::nominate(Origin::signed(2), 1, 10, 10, 10));
			let collator_state = Stake::collator_state2(1).unwrap();
			assert_eq!(collator_state.top_nominators.len(), 1usize);
			assert!(collator_state.bottom_nominators.is_empty());
			// 2 nominators => 2 top nominators, 0 bottom nominators
			assert_ok!(Stake::nominate(Origin::signed(3), 1, 10, 10, 10));
			let collator_state = Stake::collator_state2(1).unwrap();
			assert_eq!(collator_state.top_nominators.len(), 2usize);
			assert!(collator_state.bottom_nominators.is_empty());
			// 3 nominators => 3 top nominators, 0 bottom nominators
			assert_ok!(Stake::nominate(Origin::signed(4), 1, 10, 10, 10));
			let collator_state = Stake::collator_state2(1).unwrap();
			assert_eq!(collator_state.top_nominators.len(), 3usize);
			assert!(collator_state.bottom_nominators.is_empty());
			// 4 nominators => 4 top nominators, 0 bottom nominators
			assert_ok!(Stake::nominate(Origin::signed(5), 1, 10, 10, 10));
			let collator_state = Stake::collator_state2(1).unwrap();
			assert_eq!(collator_state.top_nominators.len(), 4usize);
			assert!(collator_state.bottom_nominators.is_empty());
		});
}

#[test]
// MaxNominatorsPerCollator = 4
fn candidate_pool_updates_when_total_counted_changes() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 20),
			(3, 19),
			(4, 20),
			(5, 21),
			(6, 22),
			(7, 15),
			(8, 16),
			(9, 17),
			(10, 18),
		])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![
			(3, 1, 11),
			(4, 1, 12),
			(5, 1, 13),
			(6, 1, 14),
			(7, 1, 15),
			(8, 1, 16),
			(9, 1, 17),
			(10, 1, 18),
		])
		.build()
		.execute_with(|| {
			fn is_candidate_pool_bond(account: u64, bond: u128) {
				let pool = Stake::candidate_pool();
				for candidate in pool.0 {
					if candidate.owner == account {
						assert_eq!(candidate.amount, bond);
					}
				}
			}
			// 15 + 16 + 17 + 18 + 20 = 86 (top 4 + self bond)
			is_candidate_pool_bond(1, 86);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(3), 1, 8));
			// 16 + 17 + 18 + 19 + 20 = 90 (top 4 + self bond)
			is_candidate_pool_bond(1, 90);
			assert_ok!(Stake::nominator_bond_more(Origin::signed(4), 1, 8));
			// 17 + 18 + 19 + 20 + 20 = 94 (top 4 + self bond)
			is_candidate_pool_bond(1, 94);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(10), 1, 3));
			// 16 + 17 + 19 + 20 + 20 = 92 (top 4 + self bond)
			is_candidate_pool_bond(1, 92);
			assert_ok!(Stake::nominator_bond_less(Origin::signed(9), 1, 4));
			// 15 + 16 + 19 + 20 + 20 = 90 (top 4 + self bond)
			is_candidate_pool_bond(1, 90);
		});
}

#[test]
// MaxNominatorsPerCollator = 4
fn only_top_collators_are_counted() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 20),
			(3, 19),
			(4, 20),
			(5, 21),
			(6, 22),
			(7, 15),
			(8, 16),
			(9, 17),
			(10, 18),
		])
		.with_candidates(vec![(1, 20)])
		.with_nominations(vec![
			(3, 1, 11),
			(4, 1, 12),
			(5, 1, 13),
			(6, 1, 14),
			(7, 1, 15),
			(8, 1, 16),
			(9, 1, 17),
			(10, 1, 18),
		])
		.build()
		.execute_with(|| {
			// sanity check that 3-10 are nominators immediately
			for i in 3..11 {
				assert!(Stake::is_nominator(&i));
			}
			let mut expected_events = Vec::new();
			let collator_state = Stake::collator_state2(1).unwrap();
			// 15 + 16 + 17 + 18 + 20 = 86 (top 4 + self bond)
			assert_eq!(collator_state.total_counted, 86);
			// 11 + 12 + 13 + 14 = 50
			assert_eq!(collator_state.total_counted + 50, collator_state.total_backing);
			// bump bottom to the top
			assert_ok!(Stake::nominator_bond_more(Origin::signed(3), 1, 8));
			expected_events.push(Event::NominationIncreased {
				nominator: 3,
				collator: 1,
				amount: 8,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator_state = Stake::collator_state2(1).unwrap();
			// 16 + 17 + 18 + 19 + 20 = 90 (top 4 + self bond)
			assert_eq!(collator_state.total_counted, 90);
			// 12 + 13 + 14 + 15 = 54
			assert_eq!(collator_state.total_counted + 54, collator_state.total_backing);
			// bump bottom to the top
			assert_ok!(Stake::nominator_bond_more(Origin::signed(4), 1, 8));
			expected_events.push(Event::NominationIncreased {
				nominator: 4,
				collator: 1,
				amount: 8,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator_state = Stake::collator_state2(1).unwrap();
			// 17 + 18 + 19 + 20 + 20 = 94 (top 4 + self bond)
			assert_eq!(collator_state.total_counted, 94);
			// 13 + 14 + 15 + 16 = 58
			assert_eq!(collator_state.total_counted + 58, collator_state.total_backing);
			// bump bottom to the top
			assert_ok!(Stake::nominator_bond_more(Origin::signed(5), 1, 8));
			expected_events.push(Event::NominationIncreased {
				nominator: 5,
				collator: 1,
				amount: 8,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator_state = Stake::collator_state2(1).unwrap();
			// 18 + 19 + 20 + 21 + 20 = 98 (top 4 + self bond)
			assert_eq!(collator_state.total_counted, 98);
			// 14 + 15 + 16 + 17 = 62
			assert_eq!(collator_state.total_counted + 62, collator_state.total_backing);
			// bump bottom to the top
			assert_ok!(Stake::nominator_bond_more(Origin::signed(6), 1, 8));
			expected_events.push(Event::NominationIncreased {
				nominator: 6,
				collator: 1,
				amount: 8,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator_state = Stake::collator_state2(1).unwrap();
			// 19 + 20 + 21 + 22 + 20 = 102 (top 4 + self bond)
			assert_eq!(collator_state.total_counted, 102);
			// 15 + 16 + 17 + 18 = 66
			assert_eq!(collator_state.total_counted + 66, collator_state.total_backing);
		});
}

#[test]
fn nomination_events_convey_correct_position() {
	ExtBuilder::default()
		.with_balances(vec![
			(1, 100),
			(2, 100),
			(3, 100),
			(4, 100),
			(5, 100),
			(6, 100),
			(7, 100),
			(8, 100),
			(9, 100),
			(10, 100),
		])
		.with_candidates(vec![(1, 20), (2, 20)])
		.with_nominations(vec![(3, 1, 11), (4, 1, 12), (5, 1, 13), (6, 1, 14)])
		.build()
		.execute_with(|| {
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 11 + 12 + 13 + 14 + 20 = 70 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 70);
			assert_eq!(collator1_state.total_counted, collator1_state.total_backing);
			// Top nominations are full, new highest nomination is made
			assert_ok!(Stake::nominate(Origin::signed(7), 1, 15, 10, 10));
			let mut expected_events = Vec::new();
			expected_events.push(Event::Nomination {
				nominator: 7,
				locked: 15,
				collator: 1,
				nominator_position: NominatorAdded::AddedToTop { new_total: 74 },
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 12 + 13 + 14 + 15 + 20 = 70 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 74);
			// 11 = 11
			assert_eq!(collator1_state.total_counted + 11, collator1_state.total_backing);
			// New nomination is added to the bottom
			assert_ok!(Stake::nominate(Origin::signed(8), 1, 10, 10, 10));
			expected_events.push(Event::Nomination {
				nominator: 8,
				locked: 10,
				collator: 1,
				nominator_position: NominatorAdded::AddedToBottom,
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 12 + 13 + 14 + 15 + 20 = 70 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 74);
			// 10 + 11 = 21
			assert_eq!(collator1_state.total_counted + 21, collator1_state.total_backing);
			// 8 increases nomination to the top
			assert_ok!(Stake::nominator_bond_more(Origin::signed(8), 1, 3));
			expected_events.push(Event::NominationIncreased {
				nominator: 8,
				collator: 1,
				amount: 3,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 13 + 13 + 14 + 15 + 20 = 75 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 75);
			// 11 + 12 = 23
			assert_eq!(collator1_state.total_counted + 23, collator1_state.total_backing);
			// 3 increases nomination but stays in bottom
			assert_ok!(Stake::nominator_bond_more(Origin::signed(3), 1, 1));
			expected_events.push(Event::NominationIncreased {
				nominator: 3,
				collator: 1,
				amount: 1,
				in_top: false,
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 13 + 13 + 14 + 15 + 20 = 75 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 75);
			// 12 + 12 = 24
			assert_eq!(collator1_state.total_counted + 24, collator1_state.total_backing);
			// 6 decreases nomination but stays in top
			assert_ok!(Stake::nominator_bond_less(Origin::signed(6), 1, 2));
			expected_events.push(Event::NominationDecreased {
				nominator: 6,
				collator: 1,
				amount: 2,
				in_top: true,
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 12 + 13 + 13 + 15 + 20 = 73 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 73);
			// 12 + 12 = 24
			assert_eq!(collator1_state.total_counted + 24, collator1_state.total_backing);
			// 6 decreases nomination and is bumped to bottom
			assert_ok!(Stake::nominator_bond_less(Origin::signed(6), 1, 1));
			expected_events.push(Event::NominationDecreased {
				nominator: 6,
				collator: 1,
				amount: 1,
				in_top: false,
			});
			assert_eq!(events(), expected_events);
			let collator1_state = Stake::collator_state2(1).unwrap();
			// 12 + 13 + 13 + 15 + 20 = 73 (top 4 + self bond)
			assert_eq!(collator1_state.total_counted, 73);
			// 11 + 12 = 23
			assert_eq!(collator1_state.total_counted + 23, collator1_state.total_backing);
		});
}
