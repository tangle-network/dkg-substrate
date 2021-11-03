#![cfg(test)]

use core::panic;

use super::{
	mock::{
		assert_events, new_test_ext, Balances, Bridge, Call, ChainIdentifier, Event, Origin,
		ProposalLifetime, System, Test, ENDOWED_BALANCE, RELAYER_A, RELAYER_B, RELAYER_C,
		TEST_THRESHOLD,
	},
	*,
};
use crate::mock::new_test_ext_initialized;
use frame_support::{assert_noop, assert_ok};

use crate::{self as pallet_bridge};

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

		assert_ok!(Bridge::set_resource(Origin::root(), id, method.clone()));
		assert_eq!(Bridge::resources(id), Some(method));

		assert_ok!(Bridge::set_resource(Origin::root(), id, method2.clone()));
		assert_eq!(Bridge::resources(id), Some(method2));

		assert_ok!(Bridge::remove_resource(Origin::root(), id));
		assert_eq!(Bridge::resources(id), None);
	})
}

#[test]
fn whitelist_chain() {
	new_test_ext().execute_with(|| {
		assert!(!Bridge::chain_whitelisted(0));

		assert_ok!(Bridge::whitelist_chain(Origin::root(), 0));
		assert_noop!(
			Bridge::whitelist_chain(Origin::root(), ChainIdentifier::get()),
			Error::<Test>::InvalidChainId
		);

		assert_events(vec![Event::Bridge(pallet_bridge::Event::ChainWhitelisted { chain_id: 0 })]);
	})
}

#[test]
fn set_get_threshold() {
	new_test_ext().execute_with(|| {
		assert_eq!(RelayerThreshold::<Test>::get(), 1);

		assert_ok!(Bridge::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(RelayerThreshold::<Test>::get(), TEST_THRESHOLD);

		assert_ok!(Bridge::set_threshold(Origin::root(), 5));
		assert_eq!(RelayerThreshold::<Test>::get(), 5);

		assert_events(vec![
			Event::Bridge(pallet_bridge::Event::RelayerThresholdChanged {
				new_threshold: TEST_THRESHOLD,
			}),
			Event::Bridge(pallet_bridge::Event::RelayerThresholdChanged { new_threshold: 5 }),
		]);
	})
}

#[test]
fn add_remove_relayer() {
	new_test_ext().execute_with(|| {
		assert_ok!(Bridge::set_threshold(Origin::root(), TEST_THRESHOLD,));
		assert_eq!(Bridge::relayer_count(), 0);

		assert_ok!(Bridge::add_relayer(Origin::root(), RELAYER_A));
		assert_ok!(Bridge::add_relayer(Origin::root(), RELAYER_B));
		assert_ok!(Bridge::add_relayer(Origin::root(), RELAYER_C));
		assert_eq!(Bridge::relayer_count(), 3);

		// Already exists
		assert_noop!(
			Bridge::add_relayer(Origin::root(), RELAYER_A),
			Error::<Test>::RelayerAlreadyExists
		);

		// Confirm removal
		assert_ok!(Bridge::remove_relayer(Origin::root(), RELAYER_B));
		assert_eq!(Bridge::relayer_count(), 2);
		assert_noop!(
			Bridge::remove_relayer(Origin::root(), RELAYER_B),
			Error::<Test>::RelayerInvalid
		);
		assert_eq!(Bridge::relayer_count(), 2);

		assert_events(vec![
			Event::Bridge(pallet_bridge::Event::RelayerAdded { relayer_id: RELAYER_A }),
			Event::Bridge(pallet_bridge::Event::RelayerAdded { relayer_id: RELAYER_B }),
			Event::Bridge(pallet_bridge::Event::RelayerAdded { relayer_id: RELAYER_C }),
			Event::Bridge(pallet_bridge::Event::RelayerRemoved { relayer_id: RELAYER_B }),
		]);
	})
}

fn make_proposal(r: Vec<u8>) -> mock::Call {
	Call::System(system::Call::remark { remark: r })
}

#[test]
fn create_sucessful_proposal() {
	let src_id = 1u32;
	let r_id = derive_resource_id(src_id, b"remark");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = 1;
		let proposal = make_proposal(vec![10]);

		// Create proposal (& vote)
		assert_ok!(Bridge::acknowledge_proposal(
			Origin::signed(RELAYER_A),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(Bridge::reject_proposal(
			Origin::signed(RELAYER_B),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![RELAYER_B],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes in favour
		assert_ok!(Bridge::acknowledge_proposal(
			Origin::signed(RELAYER_C),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A, RELAYER_C],
			votes_against: vec![RELAYER_B],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![
			Event::Bridge(pallet_bridge::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_A,
			}),
			Event::Bridge(pallet_bridge::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_B,
			}),
			Event::Bridge(pallet_bridge::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_C,
			}),
			Event::Bridge(pallet_bridge::Event::ProposalApproved {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
			Event::Bridge(pallet_bridge::Event::ProposalSucceeded {
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
		assert_ok!(Bridge::acknowledge_proposal(
			Origin::signed(RELAYER_A),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(Bridge::reject_proposal(
			Origin::signed(RELAYER_B),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![RELAYER_B],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes against
		assert_ok!(Bridge::reject_proposal(
			Origin::signed(RELAYER_C),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![RELAYER_B, RELAYER_C],
			status: ProposalStatus::Rejected,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(RELAYER_B), 0);
		assert_eq!(Balances::free_balance(Bridge::account_id()), ENDOWED_BALANCE);

		assert_events(vec![
			Event::Bridge(pallet_bridge::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_A,
			}),
			Event::Bridge(pallet_bridge::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_B,
			}),
			Event::Bridge(pallet_bridge::Event::VoteAgainst {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_C,
			}),
			Event::Bridge(pallet_bridge::Event::ProposalRejected {
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
		assert_ok!(Bridge::acknowledge_proposal(
			Origin::signed(RELAYER_A),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Change threshold
		assert_ok!(Bridge::set_threshold(Origin::root(), 1));

		// Attempt to execute
		assert_ok!(Bridge::eval_vote_state(
			Origin::signed(RELAYER_A),
			prop_id,
			src_id,
			Box::new(proposal.clone())
		));

		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(RELAYER_B), 0);
		assert_eq!(Balances::free_balance(Bridge::account_id()), ENDOWED_BALANCE);

		assert_events(vec![
			Event::Bridge(pallet_bridge::Event::VoteFor {
				chain_id: src_id,
				deposit_nonce: prop_id,
				who: RELAYER_A,
			}),
			Event::Bridge(pallet_bridge::Event::RelayerThresholdChanged { new_threshold: 1 }),
			Event::Bridge(pallet_bridge::Event::ProposalApproved {
				chain_id: src_id,
				deposit_nonce: prop_id,
			}),
			Event::Bridge(pallet_bridge::Event::ProposalSucceeded {
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
		assert_ok!(Bridge::acknowledge_proposal(
			Origin::signed(RELAYER_A),
			prop_id,
			src_id,
			r_id,
			Box::new(proposal.clone())
		));
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Increment enough blocks such that now == expiry
		System::set_block_number(ProposalLifetime::get() + 1);

		// Attempt to submit a vote should fail
		assert_noop!(
			Bridge::reject_proposal(
				Origin::signed(RELAYER_B),
				prop_id,
				src_id,
				r_id,
				Box::new(proposal.clone())
			),
			Error::<Test>::ProposalExpired
		);

		// Proposal state should remain unchanged
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// eval_vote_state should have no effect
		assert_noop!(
			Bridge::eval_vote_state(
				Origin::signed(RELAYER_C),
				prop_id,
				src_id,
				Box::new(proposal.clone())
			),
			Error::<Test>::ProposalExpired
		);
		let prop = Bridge::votes(src_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![RELAYER_A],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![Event::Bridge(pallet_bridge::Event::VoteFor {
			chain_id: src_id,
			deposit_nonce: prop_id,
			who: RELAYER_A,
		})]);
	})
}
