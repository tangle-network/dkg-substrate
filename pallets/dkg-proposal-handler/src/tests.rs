use crate::mock::*;
use codec::Encode;
use frame_support::{assert_err, assert_ok};
use sp_runtime::offchain::storage::{StorageRetrievalError, StorageValueRef};
use sp_std::vec::Vec;

use super::mock::DKGProposalHandler;
use dkg_runtime_primitives::{
	ChainIdType, DKGPayloadKey, EIP2930Transaction, OffchainSignedProposals, Proposal,
	ProposalAction, ProposalHandlerTrait, ProposalHeader, ProposalKind, TransactionAction,
	TransactionV2, OFFCHAIN_SIGNED_PROPOSALS, U256,
};
use sp_core::{sr25519, H256};
use sp_runtime::offchain::storage::MutateStorageError;

// *** Utility ***

fn add_proposal_to_offchain_storage(prop: Proposal) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

	let update_res: Result<OffchainSignedProposals<u64>, MutateStorageError<_, ()>> = proposals_ref
		.mutate(
			|val: Result<Option<OffchainSignedProposals<u64>>, StorageRetrievalError>| match val {
				Ok(Some(mut ser_props)) => {
					ser_props.proposals.push((vec![prop], 0));
					Ok(ser_props)
				},
				_ => {
					let mut prop_wrapper = OffchainSignedProposals::<u64>::default();
					prop_wrapper.proposals.push((vec![prop], 0));
					Ok(prop_wrapper)
				},
			},
		);

	assert_ok!(update_res);
}

fn check_offchain_proposals_num_eq(num: usize) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);
	let stored_props: Option<OffchainSignedProposals<u64>> =
		proposals_ref.get::<OffchainSignedProposals<u64>>().unwrap();
	assert_eq!(stored_props.is_some(), true);

	assert_eq!(stored_props.unwrap().proposals.len(), num);
}

// *** Tests ***

#[test]
fn handle_empty_proposal() {
	execute_test_with(|| {
		let prop: Vec<u8> = Vec::new();

		assert_err!(
			DKGProposalHandler::handle_unsigned_proposal(prop, ProposalAction::Sign(0)),
			crate::Error::<Test>::ProposalFormatInvalid
		);

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 0);
	});
}

#[test]
fn handle_unsigned_eip2930_transaction_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 1);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);
	})
}

#[test]
fn handle_anchor_update_proposal_success() {
	execute_test_with(|| {
		let proposal_raw: Vec<u8> = vec![
			0, 0, 0, 0, 0, 0, 223, 22, 158, 136, 193, 21, 177, 236, 107, 47, 234, 158, 193, 108,
			153, 64, 171, 132, 14, 7, 1, 0, 0, 0, 5, 57, 68, 52, 123, 169, 0, 0, 0, 1, 0, 0, 122,
			105, 0, 0, 0, 0, 37, 168, 34, 127, 179, 164, 10, 49, 149, 165, 172, 173, 194, 178, 58,
			98, 176, 16, 209, 39, 221, 166, 75, 249, 181, 131, 238, 94, 88, 214, 203, 31,
		];

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			proposal_raw,
			ProposalAction::Sign(0)
		));

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 1);
	})
}

#[test]
fn store_signed_proposal_offchain() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2);

		add_proposal_to_offchain_storage(signed_proposal);

		check_offchain_proposals_num_eq(1);
	})
}

#[test]
fn submit_signed_proposal_onchain_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2);

		add_proposal_to_offchain_storage(signed_proposal);

		assert_ok!(DKGProposalHandler::submit_signed_proposal_onchain(0));

		check_offchain_proposals_num_eq(0);
	});
}

#[test]
fn submit_signed_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2);

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal]
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_none(),
			true
		);

		assert_eq!(
			DKGProposalHandler::signed_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);
	});
}

#[test]
fn submit_signed_proposal_already_exists() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal.clone()]
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_none(),
			true
		);

		assert_eq!(
			DKGProposalHandler::signed_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		// Second submission
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::submit_signed_proposals(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			vec![signed_proposal]
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_none(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);
	});
}

#[test]
fn submit_signed_proposal_fail_invalid_sig() {
	execute_test_with(|| {
		let tx_eip2930 = EIP2930Transaction {
			chain_id: 0,
			nonce: U256::from(0u8),
			gas_price: U256::from(0u8),
			gas_limit: U256::from(0u8),
			action: TransactionAction::Create,
			value: U256::from(99u8),
			input: Vec::<u8>::new(),
			access_list: Vec::new(),
			odd_y_parity: false,
			r: H256::from([0u8; 32]),
			s: H256::from([0u8; 32]),
		};
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			Proposal::Unsigned { kind: ProposalKind::EVM, data: tx_v_2.encode() },
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);

		let mut invalid_sig: Vec<u8> = Vec::new();
		invalid_sig.extend_from_slice(&[0u8, 64]);
		let signed_proposal = Proposal::Signed {
			kind: ProposalKind::EVM,
			data: tx_v_2.encode(),
			signature: invalid_sig,
		};

		assert_err!(
			DKGProposalHandler::submit_signed_proposals(
				Origin::signed(sr25519::Public::from_raw([1; 32])),
				vec![signed_proposal]
			),
			crate::Error::<Test>::ProposalSignatureInvalid
		);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_some(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(
				ChainIdType::EVM(0),
				DKGPayloadKey::EVMProposal(0)
			)
			.is_none(),
			true
		);
	});
}

pub fn make_proposal<const N: usize>(prop: Proposal) -> Proposal {
	// Create the proposal Header
	let mut header = ProposalHeader::<u32> {
		resource_id: [
			1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0,
			0, 0, 1,
		],
		chain_id: ChainIdType::EVM(1),
		function_sig: [0x26, 0x57, 0x88, 0x01],
		nonce: 1,
	};
	let mut buf = vec![];
	header.encode_to(&mut buf);

	// N bytes parameter
	buf.extend_from_slice(&[0u8; N]);

	if let Proposal::Unsigned { kind, .. } = prop {
		return match kind {
			ProposalKind::TokenAdd => Proposal::Unsigned { kind, data: buf },
			ProposalKind::TokenRemove => Proposal::Unsigned { kind, data: buf },
			ProposalKind::WrappingFeeUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::ResourceIdUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::AnchorUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::RescueTokens => Proposal::Unsigned { kind, data: buf },
			ProposalKind::MaxDepositLimitUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::MinWithdrawalLimitUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::MaxExtLimitUpdate => Proposal::Unsigned { kind, data: buf },
			ProposalKind::MaxFeeLimitUpdate => Proposal::Unsigned { kind, data: buf },
			_ => panic!("Invalid proposal type"),
		}
	}

	panic!("Invalid proposal type")
}

#[test]
fn force_submit_should_fail_with_invalid_proposal_type() {
	execute_test_with(|| {
		assert_err!(
			DKGProposalHandler::force_submit_unsigned_proposal(
				Origin::root(),
				Proposal::Unsigned { kind: ProposalKind::AnchorUpdate, data: vec![] }
			),
			crate::Error::<Test>::ProposalFormatInvalid
		);
	});
}

#[test]
fn force_submit_should_work_with_valid_proposals() {
	execute_test_with(|| {
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<20>(Proposal::Unsigned { kind: ProposalKind::TokenAdd, data: vec![] })
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::TokenAddProposal(1)
			)
			.is_some(),
			true
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<20>(Proposal::Unsigned {
				kind: ProposalKind::TokenRemove,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::TokenRemoveProposal(1)
			)
			.is_some(),
			true
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<1>(Proposal::Unsigned {
				kind: ProposalKind::WrappingFeeUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::WrappingFeeUpdateProposal(1)
			)
			.is_some(),
			true
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<72>(Proposal::Unsigned {
				kind: ProposalKind::RescueTokens,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::RescueTokensProposal(1)
			)
			.is_some(),
			true
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<72>(Proposal::Unsigned {
				kind: ProposalKind::ResourceIdUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::ResourceIdUpdateProposal(1)
			)
			.is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<32>(Proposal::Unsigned {
				kind: ProposalKind::MaxDepositLimitUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::MaxDepositLimitUpdateProposal(1)
			)
			.is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<32>(Proposal::Unsigned {
				kind: ProposalKind::MinWithdrawalLimitUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::MinWithdrawLimitUpdateProposal(1)
			)
			.is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<32>(Proposal::Unsigned {
				kind: ProposalKind::MaxExtLimitUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::MaxExtLimitUpdateProposal(1)
			)
			.is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_proposal::<32>(Proposal::Unsigned {
				kind: ProposalKind::MaxFeeLimitUpdate,
				data: vec![]
			})
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(
				ChainIdType::EVM(1),
				DKGPayloadKey::MaxFeeLimitUpdateProposal(1)
			)
			.is_some(),
			true
		);
	});
}
