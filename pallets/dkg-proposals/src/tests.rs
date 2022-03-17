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
#![cfg(test)]

use super::{
	mock::{
		assert_events, new_test_ext, Balances, ChainIdentifier, DKGProposals, Event, Origin,
		ProposalLifetime, System, Test, ENDOWED_BALANCE, PROPOSER_A, PROPOSER_B, PROPOSER_C,
		PROPOSER_D, TEST_THRESHOLD,
	},
	*,
};
use crate::mock::{
	assert_has_event, manually_set_proposer_count, mock_ecdsa_key, mock_pub_key,
	new_test_ext_initialized, roll_to, CollatorSelection, DKGProposalHandler, ExtBuilder, Session,
};
use core::panic;
use dkg_runtime_primitives::{
	DKGPayloadKey, FunctionSignature, Proposal, ProposalHeader, ProposalKind, ProposalNonce,
	TypedChainId,
};
use frame_support::{assert_err, assert_noop, assert_ok};
use std::vec;

use crate as pallet_dkg_proposals;

use crate::utils::derive_resource_id;

#[test]
fn derive_ids() {
	let chain: u32 = 0xaabbccdd;
	let chain_type: u16 = 1;
	let id = [
		0x21, 0x60, 0x5f, 0x71, 0x84, 0x5f, 0x37, 0x2a, 0x9e, 0xd8, 0x42, 0x53, 0xd2, 0xd0, 0x24,
		0xb7, 0xb1, 0x09, 0x99, 0xf4,
	];
	let r_id = derive_resource_id(chain, chain_type, &id);
	let expected = [
		0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x21, 0x60, 0x5f, 0x71, 0x84, 0x5f, 0x37, 0x2a, 0x9e, 0xd8,
		0x42, 0x53, 0xd2, 0xd0, 0x24, 0xb7, 0xb1, 0x09, 0x99, 0xf4, 0x01, 0x0, 0xdd, 0xcc, 0xbb,
		0xaa,
	];
	assert_eq!(r_id.to_bytes(), expected);
}

pub const NOT_PROPOSER: u8 = 0x5;

#[test]
fn complete_proposal_approved() {
	let mut prop = ProposalVotes {
		votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)],
		votes_against: vec![mock_pub_key(PROPOSER_C)],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get() + 1,
	};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Approved);
}

#[test]
fn complete_proposal_rejected() {
	let mut prop = ProposalVotes {
		votes_for: vec![mock_pub_key(PROPOSER_A)],
		votes_against: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_B)],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get() + 1,
	};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Rejected);
}

#[test]
fn complete_proposal_bad_threshold() {
	let mut prop = ProposalVotes {
		votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)],
		votes_against: vec![],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get() + 1,
	};

	prop.try_to_complete(3, 2);
	assert_eq!(prop.status, ProposalStatus::Initiated);

	let mut prop = ProposalVotes {
		votes_for: vec![],
		votes_against: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)],
		status: ProposalStatus::Initiated,
		expiry: ProposalLifetime::get() + 1,
	};

	prop.try_to_complete(3, 2);
	assert_eq!(prop.status, ProposalStatus::Initiated);
}

#[test]
fn setup_resources() {
	new_test_ext().execute_with(|| {
		let id: ResourceId = [1; 32].into();
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
		assert!(!DKGProposals::chain_whitelisted(TypedChainId::Evm(0)));

		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), TypedChainId::Evm(0)));
		let typed_chain_id = ChainIdentifier::get();
		assert_noop!(
			DKGProposals::whitelist_chain(Origin::root(), typed_chain_id),
			Error::<Test>::InvalidChainId
		);

		assert_events(vec![Event::DKGProposals(pallet_dkg_proposals::Event::ChainWhitelisted {
			chain_id: TypedChainId::Evm(0),
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

		assert_ok!(DKGProposals::add_proposer(
			Origin::root(),
			mock_pub_key(PROPOSER_A),
			mock_ecdsa_key(PROPOSER_A)
		));
		assert_ok!(DKGProposals::add_proposer(
			Origin::root(),
			mock_pub_key(PROPOSER_B),
			mock_ecdsa_key(PROPOSER_B)
		));
		assert_ok!(DKGProposals::add_proposer(
			Origin::root(),
			mock_pub_key(PROPOSER_C),
			mock_ecdsa_key(PROPOSER_C)
		));
		assert_eq!(DKGProposals::proposer_count(), 3);

		// Already exists
		assert_noop!(
			DKGProposals::add_proposer(
				Origin::root(),
				mock_pub_key(PROPOSER_A),
				mock_ecdsa_key(PROPOSER_A)
			),
			Error::<Test>::ProposerAlreadyExists
		);

		// Confirm removal
		assert_ok!(DKGProposals::remove_proposer(Origin::root(), mock_pub_key(PROPOSER_B)));
		assert_eq!(DKGProposals::proposer_count(), 2);
		assert_noop!(
			DKGProposals::remove_proposer(Origin::root(), mock_pub_key(PROPOSER_B)),
			Error::<Test>::ProposerInvalid
		);
		assert_eq!(DKGProposals::proposer_count(), 2);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: mock_pub_key(PROPOSER_A),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: mock_pub_key(PROPOSER_B),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerAdded {
				proposer_id: mock_pub_key(PROPOSER_C),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerRemoved {
				proposer_id: mock_pub_key(PROPOSER_B),
			}),
		]);
	})
}

pub fn make_proposal<const N: usize>(prop: Proposal) -> Vec<u8> {
	// Create the proposal Header
	let r_id = ResourceId::from([
		1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
		0, 1,
	]);
	let function_signature = FunctionSignature::from([0x26, 0x57, 0x88, 0x01]);
	let nonce = ProposalNonce::from(1);
	let header = ProposalHeader::new(r_id, function_signature, nonce);
	let mut buf = vec![];
	header.encode_to(&mut buf);
	// N bytes parameter
	buf.extend_from_slice(&[0u8; N]);

	match prop {
		Proposal::Unsigned { kind: ProposalKind::AnchorUpdate, .. } =>
			Proposal::Unsigned { kind: ProposalKind::AnchorUpdate, data: buf },
		_ => panic!("Invalid proposal type"),
	}
	.data()
	.clone()
}

#[test]
fn create_successful_proposal() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.chain_id(), 0x0100, b"remark");

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal = make_proposal::<42>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![],
		});
		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(mock_pub_key(PROPOSER_B)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![mock_pub_key(PROPOSER_B)],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes in favour
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_C)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_C)],
			votes_against: vec![mock_pub_key(PROPOSER_B)],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_A),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_B),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_C),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn create_unsucessful_proposal() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.chain_id(), 0x0100, b"transfer");

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal = make_proposal::<42>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![],
		});

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(mock_pub_key(PROPOSER_B)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![mock_pub_key(PROPOSER_B)],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Third relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			Origin::signed(mock_pub_key(PROPOSER_C)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![mock_pub_key(PROPOSER_B), mock_pub_key(PROPOSER_C)],
			status: ProposalStatus::Rejected,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(mock_pub_key(PROPOSER_B)), 0);
		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_A),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_B),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_C),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalRejected {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn execute_after_threshold_change() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.chain_id(), 0x0100, b"transfer");

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal = make_proposal::<42>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![],
		});

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Change threshold
		assert_ok!(DKGProposals::set_threshold(Origin::root(), 1));

		// Attempt to execute
		assert_ok!(DKGProposals::eval_vote_state(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			proposal.clone(),
		));

		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(mock_pub_key(PROPOSER_B)), 0);
		assert_events(vec![
			Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_A),
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: 1,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
			}),
			Event::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
				chain_id: typed_chain_id,
				proposal_nonce: prop_id,
			}),
		]);
	})
}

#[test]
fn proposal_expires() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.chain_id(), 0x0100, b"remark");

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal = make_proposal::<42>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![],
		});

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Increment enough blocks such that now == expiry
		System::set_block_number(ProposalLifetime::get() + 1);

		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Attempt to submit a vote should fail
		assert_noop!(
			DKGProposals::reject_proposal(
				Origin::signed(mock_pub_key(PROPOSER_B)),
				prop_id,
				typed_chain_id,
				r_id,
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);

		// Proposal state should remain unchanged
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// eval_vote_state should have no effect
		assert_noop!(
			DKGProposals::eval_vote_state(
				Origin::signed(mock_pub_key(PROPOSER_C)),
				prop_id,
				typed_chain_id,
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);
		let prop =
			DKGProposals::votes(typed_chain_id, (prop_id.clone(), proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)],
			votes_against: vec![],
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![Event::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
			chain_id: typed_chain_id,
			proposal_nonce: prop_id,
			who: mock_pub_key(PROPOSER_A),
		})]);
	})
}

#[test]
fn should_get_initial_proposers_from_dkg() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		// Initial proposer set is invulnerables even when another collator exists
		assert_eq!(DKGProposals::proposer_count(), 3);
		// Advance a session
		roll_to(10);
		// Advance a session
		roll_to(20);
		// The fourth collator is now in the proposer set as well
		assert_eq!(DKGProposals::proposer_count(), 4);
	})
}

#[test]
fn should_reset_proposers_if_authorities_changed_during_a_session_change() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		assert_eq!(DKGProposals::proposer_count(), 3);
		// Leave proposer set before session change
		CollatorSelection::leave_intent(Origin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		// Advance a session
		roll_to(10);
		// Proposer set remains the same size
		assert_eq!(DKGProposals::proposer_count(), 3);
		assert_has_event(Event::Session(pallet_session::Event::NewSession { session_index: 1 }));
		assert_has_event(Event::DKGProposals(crate::Event::AuthorityProposersReset {
			proposers: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
		}));
	})
}

// Whenever the collator set changes, which in turn would cause the DKG authorities to change, the
// proposers to should also be changed.
#[test]
fn should_reset_proposers_if_authorities_changed() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		CollatorSelection::leave_intent(Origin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		roll_to(10);
		assert_has_event(Event::DKGProposals(crate::Event::AuthorityProposersReset {
			proposers: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
		}))
	})
}

// This tess that only accounts in the proposer set are allowed to make proposals,
//  when the authority set changes, if an authority has been removed from the set, they should not
// be able to make proposals anymore.
#[test]
fn only_current_authorities_should_make_successful_proposals() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.chain_id(), 0x0100, b"remark");

	ExtBuilder::with_genesis_collators().execute_with(|| {
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);

		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), typed_chain_id));
		// Set and check resource ID mapped to some junk data
		assert_ok!(DKGProposals::set_resource(Origin::root(), r_id, b"System.remark".to_vec()));
		assert_eq!(DKGProposals::resource_exists(r_id), true);

		let prop_id = ProposalNonce::from(1u32);
		let proposal = make_proposal::<42>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![],
		});

		assert_err!(
			DKGProposals::reject_proposal(
				Origin::signed(mock_pub_key(NOT_PROPOSER)),
				prop_id,
				typed_chain_id,
				r_id,
				proposal.clone(),
			),
			crate::Error::<Test>::MustBeProposer
		);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			Origin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			r_id,
			proposal.clone(),
		));

		CollatorSelection::leave_intent(Origin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		roll_to(10);
		assert_has_event(Event::DKGProposals(crate::Event::AuthorityProposersReset {
			proposers: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
		}));

		// Create proposal (& vote)
		assert_err!(
			DKGProposals::reject_proposal(
				Origin::signed(mock_pub_key(PROPOSER_D)),
				prop_id,
				typed_chain_id,
				r_id,
				proposal.clone(),
			),
			crate::Error::<Test>::MustBeProposer
		);
	})
}

#[test]
fn session_change_should_create_proposer_set_update_proposal() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		roll_to(40);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				TypedChainId::None,
				DKGPayloadKey::ProposerSetUpdateProposal(5.into())
			)
			.is_some(),
			true
		);

		roll_to(41);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				TypedChainId::None,
				DKGPayloadKey::ProposerSetUpdateProposal(6.into())
			)
			.is_some(),
			false
		);

		roll_to(45);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				TypedChainId::None,
				DKGPayloadKey::ProposerSetUpdateProposal(5.into())
			)
			.is_some(),
			true
		);

		roll_to(80);
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				TypedChainId::None,
				DKGPayloadKey::ProposerSetUpdateProposal(9.into())
			)
			.is_some(),
			true
		);
	})
}

#[test]
fn proposers_tree_height_should_compute_correctly() {
	manually_set_proposer_count(18).execute_with(|| {
		assert_eq!(DKGProposals::get_proposer_set_tree_height(), 5);
	});
	manually_set_proposer_count(16).execute_with(|| {
		assert_eq!(DKGProposals::get_proposer_set_tree_height(), 4);
	});
	manually_set_proposer_count(1).execute_with(|| {
		assert_eq!(DKGProposals::get_proposer_set_tree_height(), 1);
	});
	manually_set_proposer_count(2).execute_with(|| {
		assert_eq!(DKGProposals::get_proposer_set_tree_height(), 1);
	});
	manually_set_proposer_count(100).execute_with(|| {
		assert_eq!(DKGProposals::get_proposer_set_tree_height(), 7);
	});
}

#[test]
fn proposers_iter_keys_should_only_contain_active_proposers() {
	let src_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(src_id.chain_id(), 0x0100, b"remark");

	new_test_ext_initialized(src_id.clone(), r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_keys: Vec<_> = Proposers::<Test>::iter_keys().collect();
		assert_eq!(prop_keys.len(), 3);
	});
}

// TODO: Test this better...right now just printing the root
#[test]
fn should_output_valid_root() {
	let src_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(src_id.chain_id(), 0x0100, b"remark");

	new_test_ext_initialized(src_id.clone(), r_id, b"System.remark".to_vec()).execute_with(|| {
		println!("{:?}", DKGProposals::get_proposer_set_tree_root());
	});
}
use sp_runtime::app_crypto::ecdsa;
// Tests whether proposer root is correct
#[test]
fn should_calculate_corrrect_proposer_set_root() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		// Initial proposer set is invulnerables even when another collator exists
		assert_eq!(DKGProposals::proposer_count(), 3);
		// Get the three invulnerable proposers
		println!("{:?}", ecdsa::Public::from_raw([3; 33]));
		println!("{:?}", mock_ecdsa_key(3));
		// Advance a two sessions
		roll_to(20);
		// The fourth collator is now in the proposer set as well
		assert_eq!(DKGProposals::proposer_count(), 4);
	})
}

#[test]
fn shouljd_calculate_corrrect_proposer_set_root() {
	println!("{:?}", ecdsa::Public::from_raw([3; 33]));
	let x = mock_ecdsa_key(3);
	println!("{:?}", x);
}
