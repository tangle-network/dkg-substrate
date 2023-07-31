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
#![allow(clippy::unwrap_used)]
use super::mock::DKGProposalHandler;

use crate::{mock::*, Error, SignedProposalBatchOf};
use codec::Encode;
use dkg_runtime_primitives::{
	offchain::storage_keys::OFFCHAIN_SIGNED_PROPOSALS, ProposalHandlerTrait, TransactionV2,
	TypedChainId,
};
use frame_support::{
	assert_err, assert_noop, assert_ok,
	traits::{Hooks, OnFinalize},
	weights::constants::RocksDbWeight,
};
use sp_core::sr25519;
use sp_runtime::offchain::storage::{StorageRetrievalError, StorageValueRef};
use sp_std::vec::Vec;

use dkg_runtime_primitives::ProposalHeader;
use sp_runtime::offchain::storage::MutateStorageError;
use webb_proposals::{Proposal, ProposalKind};

// *** Utility ***

fn add_proposal_to_offchain_storage(prop: SignedProposalBatchOf<Test>) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

	let update_res: Result<Vec<SignedProposalBatchOf<Test>>, MutateStorageError<_, ()>> =
		proposals_ref.mutate(
			|val: Result<Option<Vec<SignedProposalBatchOf<Test>>>, StorageRetrievalError>| match val
			{
				Ok(Some(mut ser_props)) => {
					ser_props.push(prop);
					Ok(ser_props)
				},
				_ => {
					let prop_wrapper: Vec<SignedProposalBatchOf<Test>> = vec![prop];
					Ok(prop_wrapper)
				},
			},
		);

	assert_ok!(update_res);
}

fn check_offchain_proposals_num_eq(num: usize) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);
	let stored_props: Option<Vec<SignedProposalBatchOf<Test>>> =
		proposals_ref.get::<Vec<SignedProposalBatchOf<Test>>>().unwrap();
	assert!(stored_props.is_some(), "{}", true);
	assert_eq!(stored_props.unwrap().len(), num);
}

// helper function to skip blocks
pub fn run_n_blocks(n: u64) -> u64 {
	// lets leave enough weight to read a queue with length one and remove one item
	let idle_weight = RocksDbWeight::get().reads_writes(1, 1);
	let start_block = System::block_number();

	for block_number in start_block..=n {
		System::set_block_number(block_number);

		// ensure the on_idle is executed
		<frame_system::Pallet<Test>>::register_extra_weight_unchecked(
			DKGProposalHandler::on_idle(block_number, idle_weight),
			frame_support::dispatch::DispatchClass::Mandatory,
		);

		<frame_system::Pallet<Test> as OnFinalize<u64>>::on_finalize(block_number);
	}

	System::block_number()
}

// *** Tests ***

#[test]
fn handle_unsigned_eip2930_transaction_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		let unsigned_proposal = Proposal::Unsigned {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode().try_into().unwrap(),
		};

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			unsigned_proposal.clone(),
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(0),)
				.unwrap()
				.first()
				.unwrap()
				.proposal,
			unsigned_proposal
		);
	})
}

#[test]
fn handle_anchor_update_proposal_success() {
	execute_test_with(|| {
		let proposal_raw: [u8; 104] = [
			0, 0, 0, 0, 0, 0, 223, 22, 158, 136, 193, 21, 177, 236, 107, 47, 234, 158, 193, 108,
			153, 64, 171, 132, 14, 7, 1, 0, 0, 0, 5, 57, 68, 52, 123, 169, 0, 0, 0, 1, 1, 0, 0, 0,
			122, 105, 0, 0, 0, 0, 37, 168, 34, 127, 179, 164, 10, 49, 149, 165, 172, 173, 194, 178,
			181, 131, 238, 94, 88, 214, 203, 31, 58, 98, 176, 16, 209, 39, 221, 166, 75, 249, 181,
			131, 238, 94, 88, 214, 203, 31, 58, 98, 176, 16, 209, 39, 221, 166, 75, 249, 181, 131,
			238, 94,
		];

		let unsigned_proposal = Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: proposal_raw.encode().try_into().unwrap(),
		};

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(unsigned_proposal.clone()));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1337),)
				.unwrap()
				.first()
				.unwrap()
				.proposal,
			unsigned_proposal
		);
	})
}

#[test]
fn store_signed_proposal_offchain() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		let unsigned_proposal = Proposal::Unsigned {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode().try_into().unwrap(),
		};

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			unsigned_proposal,
		));

		let signed_proposal_batch = mock_signed_proposal_batch(tx_v_2);

		println!("{signed_proposal_batch:?}");

		add_proposal_to_offchain_storage(signed_proposal_batch);

		check_offchain_proposals_num_eq(1);
	})
}

#[test]
fn submit_signed_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		let unsigned_proposal = Proposal::Unsigned {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode().try_into().unwrap(),
		};

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			unsigned_proposal.clone(),
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		// the unsigned proposal queue should be empty, this is becuase
		// the ProposerSetUpdate is always a Batch-of-1, so we skip the unsigned
		// proposal queue and insert it directly to the unsigned proposal batch queue
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(0)).is_none());

		// the unsigned proposal batch should have the proposal
		assert_eq!(
			DKGProposalHandler::unsigned_proposal_queue(TypedChainId::Evm(0), 0_u32)
				.unwrap()
				.proposals
				.first()
				.unwrap()
				.proposal,
			unsigned_proposal
		);

		let signed_proposal = mock_signed_proposal_batch(tx_v_2);

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal]
		));

		assert!(
			DKGProposalHandler::unsigned_proposal_queue(TypedChainId::Evm(0), 0).is_none(),
			"{}",
			true
		);

		assert!(
			DKGProposalHandler::signed_proposals(TypedChainId::Evm(0), 0).is_some(),
			"{}",
			true
		);
	});
}

#[test]
fn submit_signed_proposal_batch_already_exists() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		let signed_proposal = mock_signed_proposal_batch(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal.clone()]
		));

		// Second submission
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		assert_noop!(
			DKGProposalHandler::submit_signed_proposals(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				vec![signed_proposal]
			),
			Error::<Test>::ProposalDoesNotExists
		);
	});
}

#[test]
fn submit_signed_proposal_fail_invalid_sig() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		let mut invalid_sig: Vec<u8> = Vec::new();
		invalid_sig.extend_from_slice(&[0u8, 64]);

		let mut signed_proposal = mock_signed_proposal_batch(tx_v_2);
		signed_proposal.signature = invalid_sig.try_into().unwrap();

		// it does not return an error, however the proposal is not added to the list.
		// This is because the signature is invalid, and we are batch processing.
		// we could check for the RuntimeEvent that is emitted.
		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal]
		));

		assert!(
			DKGProposalHandler::signed_proposals(TypedChainId::Evm(0), 1).is_none(),
			"{}",
			true
		);
	});
}

pub fn make_header(chain: TypedChainId) -> ProposalHeader {
	match chain {
		TypedChainId::Evm(_) => ProposalHeader::new(
			[
				1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0,
				0, 0, 0, 1,
			]
			.into(),
			[0x26, 0x57, 0x88, 0x01].into(),
			1.into(),
		),
		TypedChainId::Substrate(_) => ProposalHeader::new(
			[
				1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 2, 0,
				0, 0, 0, 1,
			]
			.into(),
			[0x0, 0x0, 0x0, 0x0].into(),
			1.into(),
		),
		_ => {
			// Dummy Header
			ProposalHeader::new(
				[
					1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0,
					0, 0, 0, 0, 0, 1,
				]
				.into(),
				[0x0, 0x0, 0x0, 0x0].into(),
				1.into(),
			)
		},
	}
}

pub fn make_proposal<const N: usize>(
	prop: Proposal<<Test as pallet_dkg_metadata::Config>::MaxProposalLength>,
	chain: TypedChainId,
) -> Proposal<<Test as pallet_dkg_metadata::Config>::MaxProposalLength> {
	// Create the proposal Header
	let header = make_header(chain);
	let mut buf = vec![];
	header.encode_to(&mut buf);
	// N bytes parameter
	buf.extend_from_slice(&[0u8; N]);

	if let Proposal::Unsigned { kind, .. } = prop {
		return match kind {
			ProposalKind::Refresh => Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::TokenAdd => Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::TokenRemove => Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::WrappingFeeUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::ResourceIdUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::AnchorCreate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::AnchorUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::RescueTokens =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::MaxDepositLimitUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::MinWithdrawalLimitUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::SetTreasuryHandler =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::SetVerifier => Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			ProposalKind::FeeRecipientUpdate =>
				Proposal::Unsigned { kind, data: buf.try_into().unwrap() },
			_ => panic!("Invalid proposal type"),
		}
	}

	panic!("Invalid proposal type")
}

#[test]
fn force_submit_should_fail_with_invalid_proposal_header_bytes() {
	execute_test_with(|| {
		assert_err!(
			DKGProposalHandler::force_submit_unsigned_proposal(
				RuntimeOrigin::root(),
				Proposal::Unsigned {
					kind: ProposalKind::AnchorUpdate,
					data: vec![].try_into().unwrap()
				}
			),
			crate::Error::<Test>::InvalidProposalBytesLength
		);
	});
}

#[test]
fn force_submit_should_fail_with_invalid_proposal_type() {
	execute_test_with(|| {
		assert_err!(
			DKGProposalHandler::force_submit_unsigned_proposal(
				RuntimeOrigin::root(),
				make_proposal::<20>(
					Proposal::Unsigned {
						kind: ProposalKind::Refresh,
						data: vec![].try_into().unwrap()
					},
					TypedChainId::None,
				)
			),
			crate::Error::<Test>::ProposalFormatInvalid
		);
	});
}

#[test]
fn force_submit_should_work_with_anchor_update() {
	execute_test_with(|| {
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: vec![
					0, 0, 0, 0, 0, 0, 211, 12, 136, 57, 193, 20, 86, 9, 229, 100, 185, 134, 246,
					103, 178, 115, 221, 203, 132, 150, 1, 0, 0, 0, 19, 137, 0, 0, 0, 0, 0, 0, 0, 0,
					0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
					0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 230, 154, 132, 124, 213, 188, 12, 148, 128,
					173, 160, 179, 57, 215, 240, 168, 202, 194, 182, 103, 1, 0, 0, 0, 19, 138
				]
				.try_into()
				.unwrap()
			}
		));
	});
}

#[test]
fn force_submit_should_work_with_valid_proposals() {
	execute_test_with(|| {
		// EVM Tests
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::TokenAdd,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1),).unwrap().len() == 1);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::TokenRemove,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1),).unwrap().len() == 2);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<2>(
				Proposal::Unsigned {
					kind: ProposalKind::WrappingFeeUpdate,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1),).unwrap().len() == 3);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<72>(
				Proposal::Unsigned {
					kind: ProposalKind::RescueTokens,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1),).unwrap().len() == 4);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<52>(
				Proposal::Unsigned {
					kind: ProposalKind::ResourceIdUpdate,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));
		assert!(DKGProposalHandler::unsigned_proposals(TypedChainId::Evm(1),).unwrap().len() == 5);

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<32>(
				Proposal::Unsigned {
					kind: ProposalKind::MaxDepositLimitUpdate,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<32>(
				Proposal::Unsigned {
					kind: ProposalKind::MinWithdrawalLimitUpdate,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::SetTreasuryHandler,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::SetVerifier,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::FeeRecipientUpdate,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Evm(0)
			)
		));

		// Substrate Tests
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::TokenAdd,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Substrate(0)
			)
		));

		assert!(
			DKGProposalHandler::unsigned_proposals(TypedChainId::Substrate(1),)
				.unwrap()
				.len() == 1
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			make_proposal::<20>(
				Proposal::Unsigned {
					kind: ProposalKind::TokenRemove,
					data: vec![].try_into().unwrap()
				},
				TypedChainId::Substrate(0)
			)
		));
	});
}

#[test]
fn offence_reporting_rejects_an_existing_proposal() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		let mut signed_proposal = mock_signed_proposal_batch(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal.clone()]
		));

		// Report offence on a valid signed batch should be rejected
		assert_noop!(
			DKGProposalHandler::submit_dkg_signing_offence(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal.clone()
			),
			Error::<Test>::ProposalExistsAndIsValid
		);

		// Report offence on a valid signed batch with batch_id changed should be rejected
		signed_proposal.batch_id = 999;
		assert_noop!(
			DKGProposalHandler::submit_dkg_signing_offence(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal.clone()
			),
			Error::<Test>::ProposalExistsAndIsValid
		);

		// Should reject a signed proposal without any proposals
		signed_proposal.proposals = vec![].try_into().unwrap();
		assert_noop!(
			DKGProposalHandler::submit_dkg_signing_offence(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal
			),
			Error::<Test>::InvalidSignedData
		);
	});
}

#[test]
fn offence_reporting_rejects_malformed_proposal() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		let mut signed_proposal = mock_signed_proposal_batch(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal.clone()]
		));

		// Should reject a signed proposal with extra data inserted
		let unsigned_proposal_1 = Proposal::Unsigned {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode().try_into().unwrap(),
		};
		let unsigned_proposal_2 = Proposal::Unsigned {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode().try_into().unwrap(),
		};
		signed_proposal.proposals =
			vec![unsigned_proposal_1, unsigned_proposal_2].try_into().unwrap();
		assert_noop!(
			DKGProposalHandler::submit_dkg_signing_offence(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal
			),
			Error::<Test>::NotSignedByCurrentDKG
		);
	});
}

#[test]
fn offence_reporting_rejects_proposal_in_current_unsigned_queue() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		let signed_proposal = mock_signed_proposal_batch(tx_v_2.clone());

		// the signed proposal was never added to a batch but is waiting in unsigned queue
		// this should be rejected
		assert_noop!(
			DKGProposalHandler::submit_dkg_signing_offence(
				RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal.clone()
			),
			Error::<Test>::ProposalExistsAndIsValid
		);

		// as a sanity test, clean unsigned queue and this should be accepted
		#[allow(deprecated)]
		crate::UnsignedProposalQueue::<Test>::remove_prefix(TypedChainId::Evm(0), None);

		assert_ok!(DKGProposalHandler::submit_dkg_signing_offence(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			signed_proposal
		));
	});
}

#[test]
fn offence_reporting_accepts_proposal_signed_not_in_queue() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			RuntimeOrigin::root(),
			Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx_v_2.encode().try_into().unwrap()
			},
		));

		// lets time travel to 5 blocks later and ensure a batch is created
		run_n_blocks(5);

		let signed_proposal = mock_signed_proposal_batch(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal.clone()]
		));

		// clean the queue
		#[allow(deprecated)]
		crate::SignedProposals::<Test>::remove_prefix(TypedChainId::Evm(0), None);

		// Report offence on a non existent signed batch should be accepted
		assert_ok!(DKGProposalHandler::submit_dkg_signing_offence(
			RuntimeOrigin::signed(sr25519::Public::from_raw([1; 32])),
			signed_proposal.clone()
		));

		let current_offences_reported = Offences::get();
		assert_eq!(
			current_offences_reported,
			vec![(
				vec![],
				crate::DKGMisbehaviourOffence {
					session_index: 0,
					validator_set_count: 0,
					offence: crate::DKGMisbehaviorOffenceType::SignedProposalNotInQueue,
					offenders: [].into()
				}
			)]
		);
	});
}
