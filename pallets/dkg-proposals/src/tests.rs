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
#![allow(clippy::unwrap_used)]
use super::{
	mock::{
		assert_events, new_test_ext, AccountId, Balances, ChainIdentifier, DKGProposals,
		ProposalLifetime, RuntimeEvent, RuntimeOrigin, System, Test, PROPOSER_A, PROPOSER_B,
		PROPOSER_C, PROPOSER_D, PROPOSER_E, TEST_THRESHOLD,
	},
	*,
};
use crate::mock::{
	assert_has_event, mock_ecdsa_address, mock_pub_key, new_test_ext_initialized, roll_to,
	CollatorSelection, ExtBuilder, MaxProposers,
};
use codec::Encode;
use core::panic;
use dkg_runtime_primitives::{
	DKGPayloadKey, FunctionSignature, MaxKeyLength, ProposalHeader, ProposalNonce, TypedChainId,
};
use frame_support::{assert_err, assert_noop, assert_ok};
use std::vec;
use webb_proposals::{Proposal, ProposalKind};

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
	let mut prop =
		ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
			votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![mock_pub_key(PROPOSER_C)].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Approved);
}

#[test]
fn complete_proposal_rejected() {
	let mut prop =
		ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_B)]
				.try_into()
				.unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};

	prop.try_to_complete(2, 3);
	assert_eq!(prop.status, ProposalStatus::Rejected);
}

#[test]
fn complete_proposal_bad_threshold() {
	let mut prop =
		ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
			votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};

	prop.try_to_complete(3, 2);
	assert_eq!(prop.status, ProposalStatus::Initiated);

	let mut prop =
		ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
			votes_for: vec![].try_into().unwrap(),
			votes_against: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_A)]
				.try_into()
				.unwrap(),
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
		let method: BoundedVec<_, _> =
			"Pallet.do_something".as_bytes().to_vec().try_into().unwrap();
		let method2: BoundedVec<_, _> =
			"Pallet.do_somethingElse".as_bytes().to_vec().try_into().unwrap();

		assert_ok!(DKGProposals::set_resource(RuntimeOrigin::root(), id, method.clone().into()));
		assert_eq!(DKGProposals::resources(id), Some(method));

		assert_ok!(DKGProposals::set_resource(RuntimeOrigin::root(), id, method2.clone().into()));
		assert_eq!(DKGProposals::resources(id), Some(method2));

		assert_ok!(DKGProposals::remove_resource(RuntimeOrigin::root(), id));
		assert_eq!(DKGProposals::resources(id), None);
	})
}

#[test]
fn whitelist_chain() {
	new_test_ext().execute_with(|| {
		assert!(!DKGProposals::chain_whitelisted(TypedChainId::Evm(0)));

		assert_ok!(DKGProposals::whitelist_chain(RuntimeOrigin::root(), TypedChainId::Evm(0)));
		let typed_chain_id = ChainIdentifier::get();
		assert_noop!(
			DKGProposals::whitelist_chain(RuntimeOrigin::root(), typed_chain_id),
			Error::<Test>::InvalidChainId
		);

		assert_events(vec![RuntimeEvent::DKGProposals(
			pallet_dkg_proposals::Event::ChainWhitelisted { chain_id: TypedChainId::Evm(0) },
		)]);
	})
}

#[test]
fn set_get_threshold() {
	new_test_ext().execute_with(|| {
		assert_eq!(ProposerThreshold::<Test>::get(), 1);

		let proposers = vec![
			(mock_pub_key(PROPOSER_A), mock_ecdsa_address(PROPOSER_A)),
			(mock_pub_key(PROPOSER_B), mock_ecdsa_address(PROPOSER_B)),
			(mock_pub_key(PROPOSER_C), mock_ecdsa_address(PROPOSER_C)),
		];
		let bounded_proposers: BoundedVec<AccountId, MaxProposers> = proposers
			.iter()
			.map(|x| x.0)
			.collect::<Vec<AccountId>>()
			.try_into()
			.expect("Too many proposers");
		Proposers::<Test>::put(bounded_proposers);
		let bounded_external_accounts: VoterList<Test> = proposers
			.iter()
			.map(|x| (x.0, x.1.clone().try_into().expect("Key size too large")))
			.collect::<Vec<(AccountId, BoundedVec<u8, MaxKeyLength>)>>()
			.try_into()
			.expect("Too many proposers");
		VotingKeys::<Test>::put(bounded_external_accounts);

		assert_ok!(DKGProposals::set_threshold(RuntimeOrigin::root(), TEST_THRESHOLD));
		assert_eq!(ProposerThreshold::<Test>::get(), TEST_THRESHOLD);

		assert_ok!(DKGProposals::set_threshold(RuntimeOrigin::root(), 3));
		assert_eq!(ProposerThreshold::<Test>::get(), 3);

		assert_events(vec![
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: TEST_THRESHOLD,
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: 3,
			}),
		]);
	})
}

pub fn make_proposal_header(
	nonce: ProposalNonce,
	r_id: ResourceId,
	function_signature: FunctionSignature,
) -> ProposalHeader {
	ProposalHeader::new(r_id, function_signature, nonce)
}

pub fn make_proposal<const N: usize>(
	header: ProposalHeader,
	prop: Proposal<<Test as pallet_dkg_proposal_handler::Config>::MaxProposalLength>,
) -> Proposal<<Test as pallet_dkg_proposal_handler::Config>::MaxProposalLength> {
	let mut buf = vec![];
	header.encode_to(&mut buf);
	// N bytes parameter
	buf.extend_from_slice(&[0u8; N]);

	match prop {
		Proposal::Unsigned { kind: ProposalKind::AnchorUpdate, .. } =>
			Proposal::<<Test as pallet_dkg_metadata::Config>::MaxProposalLength>::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: buf.try_into().unwrap(),
			},
		_ => panic!("Invalid proposal type"),
	}
}

#[test]
fn test_invalid_proposal_is_rejected() {
	let typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"System.remark");

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let _prop_id = ProposalNonce::from(1u32);
		let proposal = Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![].try_into().unwrap(),
		};

		// The proposal is invalid and should be rejected
		assert_noop!(
			DKGProposals::acknowledge_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
				proposal.clone(),
			),
			Error::<Test>::InvalidProposal
		);

		// The proposal is invalid and should be rejected
		assert_noop!(
			DKGProposals::reject_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
				proposal,
			),
			Error::<Test>::InvalidProposal
		);
	});
}

#[test]
fn create_successful_proposal() {
	let mut typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"System.remark");
	typed_chain_id = r_id.typed_chain_id();

	new_test_ext_initialized(r_id.typed_chain_id(), r_id, b"System.remark".to_vec()).execute_with(
		|| {
			let prop_id = ProposalNonce::from(1u32);

			let proposal_header = make_proposal_header(
				prop_id,
				r_id,
				FunctionSignature::from([0x26, 0x57, 0x88, 0x01]),
			);

			let proposal = make_proposal::<64>(
				proposal_header,
				Proposal::Unsigned {
					kind: ProposalKind::AnchorUpdate,
					data: vec![].try_into().unwrap(),
				},
			);

			// Create proposal (& vote)
			assert_ok!(DKGProposals::acknowledge_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
				proposal.clone(),
			));

			let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
			let expected = ProposalVotes {
				votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
				votes_against: vec![].try_into().unwrap(),
				status: ProposalStatus::Initiated,
				expiry: ProposalLifetime::get() + 1,
			};
			assert_eq!(prop, expected);

			// Second relayer votes against
			assert_ok!(DKGProposals::reject_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_B)),
				proposal.clone(),
			));
			let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
			let expected = ProposalVotes {
				votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
				votes_against: vec![mock_pub_key(PROPOSER_B)].try_into().unwrap(),
				status: ProposalStatus::Initiated,
				expiry: ProposalLifetime::get() + 1,
			};
			assert_eq!(prop, expected);

			// Third relayer votes in favour
			assert_ok!(DKGProposals::acknowledge_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_C)),
				proposal.clone(),
			));
			let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
			let expected = ProposalVotes {
				votes_for: vec![mock_pub_key(PROPOSER_A), mock_pub_key(PROPOSER_C)]
					.try_into()
					.unwrap(),
				votes_against: vec![mock_pub_key(PROPOSER_B)].try_into().unwrap(),
				status: ProposalStatus::Approved,
				expiry: ProposalLifetime::get() + 1,
			};
			assert_eq!(prop, expected);

			assert_events(vec![
				RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
					src_chain_id: typed_chain_id,
					proposal_nonce: prop_id,
					kind: proposal.kind(),
					who: mock_pub_key(PROPOSER_A),
				}),
				RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
					src_chain_id: typed_chain_id,
					proposal_nonce: prop_id,
					kind: proposal.kind(),
					who: mock_pub_key(PROPOSER_B),
				}),
				RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
					src_chain_id: typed_chain_id,
					proposal_nonce: prop_id,
					kind: proposal.kind(),
					who: mock_pub_key(PROPOSER_C),
				}),
				RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
					src_chain_id: typed_chain_id,
					kind: proposal.kind(),
					proposal_nonce: prop_id,
				}),
				RuntimeEvent::DKGProposalHandler(
					pallet_dkg_proposal_handler::Event::ProposalAdded {
						key: DKGPayloadKey::AnchorUpdateProposal(prop_id),
						target_chain: typed_chain_id,
						data: proposal.data().clone(),
					},
				),
				RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
					src_chain_id: typed_chain_id,
					proposal_nonce: prop_id,
					kind: proposal.kind(),
				}),
			]);
		},
	)
}

#[test]
fn create_unsucessful_proposal() {
	let mut typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"transfer");
	typed_chain_id = r_id.typed_chain_id();

	new_test_ext_initialized(typed_chain_id, r_id, b"transfer".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal_header =
			make_proposal_header(prop_id, r_id, FunctionSignature::from([0x26, 0x57, 0x88, 0x01]));

		let proposal = make_proposal::<64>(
			proposal_header,
			Proposal::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: vec![].try_into().unwrap(),
			},
		);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
			proposal.clone(),
		));
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Second relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_B)),
			proposal.clone(),
		));
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected =
			ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
				votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
				votes_against: vec![mock_pub_key(PROPOSER_B)].try_into().unwrap(),
				status: ProposalStatus::Initiated,
				expiry: ProposalLifetime::get() + 1,
			};
		assert_eq!(prop, expected);

		// Third relayer votes against
		assert_ok!(DKGProposals::reject_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_C)),
			proposal.clone(),
		));
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected =
			ProposalVotes::<AccountId, u64, <Test as pallet_dkg_proposals::Config>::MaxVotes> {
				votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
				votes_against: vec![mock_pub_key(PROPOSER_B), mock_pub_key(PROPOSER_C)]
					.try_into()
					.unwrap(),
				status: ProposalStatus::Rejected,
				expiry: ProposalLifetime::get() + 1,
			};

		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(mock_pub_key(PROPOSER_B)), 0);
		assert_events(vec![
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				src_chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				kind: proposal.kind(),
				who: mock_pub_key(PROPOSER_A),
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				src_chain_id: typed_chain_id,
				kind: proposal.kind(),
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_B),
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteAgainst {
				src_chain_id: typed_chain_id,
				kind: proposal.kind(),
				proposal_nonce: prop_id,
				who: mock_pub_key(PROPOSER_C),
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposalRejected {
				src_chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				kind: proposal.kind(),
			}),
		]);
	})
}

#[test]
fn execute_after_threshold_change() {
	let mut typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"transfer");
	typed_chain_id = r_id.typed_chain_id();

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal_header =
			make_proposal_header(prop_id, r_id, FunctionSignature::from([0x26, 0x57, 0x88, 0x01]));

		let proposal = make_proposal::<64>(
			proposal_header,
			Proposal::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: vec![].try_into().unwrap(),
			},
		);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
			proposal.clone(),
		));
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Change threshold
		assert_ok!(DKGProposals::set_threshold(RuntimeOrigin::root(), 1));

		// Attempt to execute
		assert_ok!(DKGProposals::eval_vote_state(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
			prop_id,
			typed_chain_id,
			proposal.clone(),
		));

		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Approved,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_eq!(Balances::free_balance(mock_pub_key(PROPOSER_B)), 0);
		assert_events(vec![
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
				src_chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				kind: proposal.kind(),
				who: mock_pub_key(PROPOSER_A),
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposerThresholdChanged {
				new_threshold: 1,
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposalApproved {
				src_chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				kind: proposal.kind(),
			}),
			RuntimeEvent::DKGProposalHandler(pallet_dkg_proposal_handler::Event::ProposalAdded {
				key: DKGPayloadKey::AnchorUpdateProposal(prop_id),
				target_chain: typed_chain_id,
				data: proposal.data().clone(),
			}),
			RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::ProposalSucceeded {
				src_chain_id: typed_chain_id,
				proposal_nonce: prop_id,
				kind: proposal.kind(),
			}),
		]);
	})
}

#[test]
fn proposal_expires() {
	let mut typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"remark");
	typed_chain_id = r_id.typed_chain_id();

	new_test_ext_initialized(typed_chain_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		let prop_id = ProposalNonce::from(1u32);
		let proposal_header =
			make_proposal_header(prop_id, r_id, FunctionSignature::from([0x26, 0x57, 0x88, 0x01]));

		let proposal = make_proposal::<64>(
			proposal_header,
			Proposal::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: vec![].try_into().unwrap(),
			},
		);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
			proposal.clone(),
		));
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Increment enough blocks such that now == expiry
		System::set_block_number(ProposalLifetime::get() + 1);

		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// Attempt to submit a vote should fail
		assert_noop!(
			DKGProposals::reject_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_B)),
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);

		// Proposal state should remain unchanged
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		// eval_vote_state should have no effect
		assert_noop!(
			DKGProposals::eval_vote_state(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_C)),
				prop_id,
				typed_chain_id,
				proposal.clone(),
			),
			Error::<Test>::ProposalExpired
		);
		let prop = DKGProposals::votes(typed_chain_id, (prop_id, proposal.clone())).unwrap();
		let expected = ProposalVotes {
			votes_for: vec![mock_pub_key(PROPOSER_A)].try_into().unwrap(),
			votes_against: vec![].try_into().unwrap(),
			status: ProposalStatus::Initiated,
			expiry: ProposalLifetime::get() + 1,
		};
		assert_eq!(prop, expected);

		assert_events(vec![RuntimeEvent::DKGProposals(pallet_dkg_proposals::Event::VoteFor {
			src_chain_id: typed_chain_id,
			proposal_nonce: prop_id,
			kind: proposal.kind(),
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
		CollatorSelection::leave_intent(RuntimeOrigin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		// Advance a session
		roll_to(10);
		// Proposer set remains the same size
		assert_eq!(DKGProposals::proposer_count(), 3);
		assert_has_event(RuntimeEvent::Session(pallet_session::Event::NewSession {
			session_index: 1,
		}));
		assert_has_event(RuntimeEvent::DKGProposals(crate::Event::ProposersReset {
			proposers: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
		}));
	})
}

// Whenever the collator set changes, which in turn would cause the DKG authorities to change,
// proposers to should also be changed.
#[test]
fn should_reset_proposers_if_authorities_changed() {
	ExtBuilder::with_genesis_collators().execute_with(|| {
		CollatorSelection::leave_intent(RuntimeOrigin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		roll_to(10);
		assert_has_event(RuntimeEvent::DKGProposals(crate::Event::ProposersReset {
			proposers: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
		}))
	})
}

// This tess that only accounts in the proposer set are allowed to make proposals,
//  when the authority set changes, if an authority has been removed from the set, they should
// not be able to make proposals anymore.
#[test]
fn only_current_authorities_should_make_successful_proposals() {
	let mut typed_chain_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(typed_chain_id.underlying_chain_id(), 0x0100, b"remark");
	typed_chain_id = r_id.typed_chain_id();

	ExtBuilder::with_genesis_collators().execute_with(|| {
		assert_ok!(DKGProposals::set_threshold(RuntimeOrigin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);

		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(RuntimeOrigin::root(), typed_chain_id));
		// Set and check resource ID mapped to some junk data
		assert_ok!(DKGProposals::set_resource(
			RuntimeOrigin::root(),
			r_id,
			b"System.remark".to_vec()
		));
		assert!(DKGProposals::resource_exists(r_id), "{}", true);

		let prop_id = ProposalNonce::from(1u32);
		let proposal_header =
			make_proposal_header(prop_id, r_id, FunctionSignature::from([0x26, 0x57, 0x88, 0x01]));
		let proposal = make_proposal::<64>(
			proposal_header,
			Proposal::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: vec![].try_into().unwrap(),
			},
		);

		assert_err!(
			DKGProposals::reject_proposal(
				RuntimeOrigin::signed(mock_pub_key(NOT_PROPOSER)),
				proposal.clone(),
			),
			crate::Error::<Test>::MustBeProposer
		);

		// Create proposal (& vote)
		assert_ok!(DKGProposals::acknowledge_proposal(
			RuntimeOrigin::signed(mock_pub_key(PROPOSER_A)),
			proposal.clone(),
		));

		CollatorSelection::leave_intent(RuntimeOrigin::signed(mock_pub_key(PROPOSER_D))).unwrap();
		// Create proposal (& vote)
		assert_err!(
			DKGProposals::reject_proposal(
				RuntimeOrigin::signed(mock_pub_key(PROPOSER_E)),
				proposal,
			),
			crate::Error::<Test>::MustBeProposer
		);
	})
}

#[test]
fn proposers_iter_keys_should_only_contain_active_proposers() {
	let src_id = TypedChainId::Evm(1);
	let r_id = derive_resource_id(src_id.underlying_chain_id(), 0x0100, b"remark");

	new_test_ext_initialized(src_id, r_id, b"System.remark".to_vec()).execute_with(|| {
		assert_eq!(Proposers::<Test>::get().len(), 3);
	});
}
