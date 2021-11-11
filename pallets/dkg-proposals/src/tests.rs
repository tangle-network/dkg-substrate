#![cfg(test)]

use core::panic;
use std::vec;

use super::{
	mock::{
		assert_events, new_test_ext, Balances, ChainIdentifier, DKGProposals, Event, Origin,
		ProposalLifetime, System, Test, ENDOWED_BALANCE, PROPOSER_A, PROPOSER_B, PROPOSER_C,
		TEST_THRESHOLD,
	},
	*,
};
use crate::mock::{
	assert_does_not_have_event, assert_has_event, new_test_ext_initialized, roll_to, ExtBuilder,
	ParachainStaking,
};
use frame_support::{assert_err, assert_noop, assert_ok};

use crate::{self as pallet_dkg_proposals};

use crate::utils::derive_resource_id;

#[test]
fn derive_ids() {
	let chain: u32 = 0xaabbccdd;
	let id = [
		0x21, 0x60, 0x5f, 0x71, 0x84, 0x5f, 0x37, 0x2a, 0x9e, 0xd8, 0x42, 0x53, 0xd2, 0xd0, 0x24,
		0xb7, 0xb1, 0x09, 0x99, 0xf4,
	];
	let r_id = derive_resource_id(chain, &id);
	let expected = [
		0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x21, 0x60, 0x5f, 0x71, 0x84, 0x5f, 0x37, 0x2a,
		0x9e, 0xd8, 0x42, 0x53, 0xd2, 0xd0, 0x24, 0xb7, 0xb1, 0x09, 0x99, 0xf4, 0xdd, 0xcc, 0xbb,
		0xaa,
	];
	assert_eq!(r_id, expected);
}

#[test]
fn complete_proposal_approved() {
	let mut prop = ProposalVotes {
		votes_for: vec![1, 2],
		votes_against: vec![3],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get(),
	};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Approved);
}

#[test]
fn complete_proposal_rejected() {
	let mut prop = ProposalVotes {
		votes_for: vec![1],
		votes_against: vec![2, 3],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get(),
	};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Rejected);
}

#[test]
fn complete_proposal_bad_threshold() {
	let mut prop = ProposalVotes {
		votes_for: vec![1, 2],
		votes_against: vec![],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get(),
	};

	prop.try_to_complete(3, 2);
	assert_eq!(prop.status, ProposalStatus::Initiated);

	let mut prop = ProposalVotes {
		votes_for: vec![],
		votes_against: vec![1, 2],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get(),
	};

	prop.try_to_complete(3, 2);
	assert_eq!(prop.status, ProposalStatus::Initiated);
}

#[test]
fn setup_resources() {
	new_test_ext().execute_with(|| {
		let id: ResourceId = [1; 32];
		let method = "Pallet.do_something".as_bytes().to_vec();
		let method2 = "Pallet.do_somethingElse".as_bytes().to_vec();

		assert_ok!(DKGProposals::set_resource(Origin::root(), id, method.clone()));
		assert_eq!(DKGProposals::resources(id), Some(method));

		assert_ok!(DKGProposals::set_resource(Origin::root(), id, method2.clone()));
		assert_eq!(DKGProposals::resources(id), Some(method2));

		assert_ok!(DKGProposals::remove_resource(Origin::root(), id));
		assert_eq!(DKGProposals::resources(id), None);
	})
}

#[test]
fn whitelist_chain() {
	new_test_ext().execute_with(|| {
		assert!(!DKGProposals::chain_whitelisted(0));

		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), 0));
		assert_noop!(
			DKGProposals::whitelist_chain(Origin::root(), ChainIdentifier::get()),
			Error::<Test>::InvalidChainId
		);

		assert_events(vec![Event::DKGProposals(pallet_dkg_proposals::Event::ChainWhitelisted {
			chain_id: 0,
		})]);
	})
}

#[test]
fn set_get_threshold() {
	new_test_ext().execute_with(|| {
		assert_eq!(ProposerThreshold::<Test>::get(), 1);

		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(ProposerThreshold::<Test>::get(), TEST_THRESHOLD);

		assert_ok!(DKGProposals::set_threshold(Origin::root(), 5));
		assert_eq!(ProposerThreshold::<Test>::get(), 5);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: TEST_THRESHOLD,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: 5,
			}),
		]);
	})
}

#[test]
fn add_remove_relayer() {
	new_test_ext().execute_with(|| {
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD,));
		assert_eq!(DKGProposals::proposer_count(), 0);

		assert_ok!(DKGProposals::add_proposer(Origin::root(), PROPOSER_A));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), PROPOSER_B));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), PROPOSER_C));
		assert_eq!(DKGProposals::proposer_count(), 3);

		// Already exists
		assert_noop!(
			DKGProposals::add_proposer(Origin::root(), PROPOSER_A),
			Error::<Test>::ProposerAlreadyExists
		);

		// Confirm removal
		assert_ok!(DKGProposals::remove_proposer(Origin::root(), PROPOSER_B));
		assert_eq!(DKGProposals::proposer_count(), 2);
		assert_noop!(
			DKGProposals::remove_proposer(Origin::root(), PROPOSER_B),
			Error::<Test>::ProposerInvalid
		);
		assert_eq!(DKGProposals::proposer_count(), 2);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: PROPOSER_A,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: PROPOSER_B,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: PROPOSER_C,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerRemoved {
				proposer_id: PROPOSER_B,
			}),
		]);
	})
}

fn make_proposal(r: Vec<u8>) -> Vec<u8> {
	r
}

#[test]
fn create_sucessful_proposal() {
	let src_id = 1u32;
	let r_id = derive_resource_id(src_id, b"remark");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = 1;
		let proposal = make_proposal(vec![10]);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(PROPOSER_A),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(PROPOSER_B),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![PROPOSER_B],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes in favour
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(PROPOSER_C),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A, PROPOSER_C],
			votes_against: vec![PROPOSER_B],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_A,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_B,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_C,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn create_unsucessful_proposal() {
	let src_id = 1;
	let r_id = derive_resource_id(src_id, b"transfer");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = 1;
		let proposal = make_proposal(vec![11]);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(PROPOSER_A),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(PROPOSER_B),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![PROPOSER_B],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(PROPOSER_C),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![PROPOSER_B, PROPOSER_C],
			status: ProposalStatus::Rejected,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(PROPOSER_B), 0);
		assert_eq!(Balances::free_balance(DKGProposals::account_id()), ENDOWED_BALANCE);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_A,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_B,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_C,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalRejected {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn execute_after_threshold_change() {
	let src_id = 1;
	let r_id = derive_resource_id(src_id, b"transfer");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = 1;
		let proposal = make_proposal(vec![11]);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(PROPOSER_A),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Change threshold
		assert_ok!(DKGProposals::set_threshold(Origin::root(), 1));

		// Attempt to execute
		assert_ok!(DKGProposals::eval_vote_state(
			Origin::signed(PROPOSER_A),
			prop_id,
			src_id,
			proposal.clone(),
		));

		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(PROPOSER_B), 0);
		assert_eq!(Balances::free_balance(DKGProposals::account_id()), ENDOWED_BALANCE);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: PROPOSER_A,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: 1,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn proposal_expires() {
	let src_id = 1;
	let r_id = derive_resource_id(src_id, b"remark");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = 1;
		let proposal = make_proposal(vec![10]);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(PROPOSER_A),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Increment enough blocks such that now == expiry
		System::set_block_number(ProposalLifetime::get() + 1);

		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Attempt to submit a vote should fail
		assert_noop!(
			DKGProposals::reject_proposal(
				Origin::signed(PROPOSER_B),
				prop_id,
				src_id,
				r_id,
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);

		// Proposal state should remain unchanged
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// eval_vote_state should have no effect
		assert_noop!(
			DKGProposals::eval_vote_state(
				Origin::signed(PROPOSER_C),
				prop_id,
				src_id,
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);
		let prop = DKGProposals::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![PROPOSER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
			chain_id: src_id,
			deposit_nonce: prop_id,
			who: PROPOSER_A,
		})]);
	})
}

#[test]
fn should_get_initial_proposers_from_dkg() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		assert_eq!(DKGProposals::proposer_count(), 4);
	})
}

#[test]
fn should_not_reset_proposers_if_authorities_have_not_changed() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		roll_to(15);
		assert_does_not_have_event(Event::DKGProposals(crate::Event::ProposersReset {
			proposers: vec![0, 1, 2, 3],
		}))
	})
}

#[test]
fn should_reset_proposers_if_authorities_changed() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		ParachainStaking::leave_candidates(Origin::signed(1), 4).unwrap();
		roll_to(15);
		assert_has_event(Event::DKGProposals(crate::Event::ProposersReset {
			proposers: vec![0, 2, 3],
		}))
	})
}

#[test]
fn only_current_authorities_should_make_successful_proposals() {
	let src_id = 1u32;
	let r_id = derive_resource_id(src_id, b"remark");

	ExtBuilder::with_genesis_collators().execute_with(|| {
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);

		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), src_id));
		// Set and check resource ID mapped to some junk data
		assert_ok!(DKGProposals::set_resource(Origin::root(), r_id, b"System.remark".to_vec()));
		assert_eq!(DKGProposals::resource_exists(r_id), true);

		let prop_id = 1;
		let proposal = make_proposal(vec![10]);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(1),
			prop_id,
			src_id,
			r_id,
			proposal.clone(),
		));

		ParachainStaking::leave_candidates(Origin::signed(1), 4).unwrap();
		roll_to(15);
		assert_has_event(Event::DKGProposals(crate::Event::ProposersReset {
			proposers: vec![0, 2, 3],
		}));

		// Create proposal (& vote)
		assert_err!(
			DKGProposals::reject_proposal(
				Origin::signed(1),
				prop_id,
				src_id,
				r_id,
				proposal.clone(),
			),
			crate::Error::<Test>::MustBeProposer
		);
	})
}
