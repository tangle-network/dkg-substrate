use crate::mock::*;
use codec::Encode;
use frame_support::{assert_err, assert_ok};
use sp_runtime::offchain::storage::{StorageRetrievalError, StorageValueRef};
use sp_std::vec::Vec;

use super::mock::DKGProposalHandler;
use dkg_runtime_primitives::{
	DKGPayloadKey, EIP2930Transaction, OffchainSignedProposals, ProposalAction,
	ProposalHandlerTrait, ProposalType, TransactionAction, TransactionV2,
	OFFCHAIN_SIGNED_PROPOSALS, U256,
};
use sp_core::{sr25519, H256};
use sp_runtime::offchain::storage::MutateStorageError;

// *** Utility ***

fn add_proposal_to_offchain_storage(prop: ProposalType) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

	let update_res: Result<OffchainSignedProposals, MutateStorageError<_, ()>> = proposals_ref
		.mutate(|val: Result<Option<OffchainSignedProposals>, StorageRetrievalError>| match val {
			Ok(Some(mut ser_props)) => {
				ser_props.proposals.push_back(prop);
				Ok(ser_props)
			},
			_ => {
				let mut prop_wrapper = OffchainSignedProposals::default();
				prop_wrapper.proposals.push_back(prop);
				Ok(prop_wrapper)
			},
		});

	assert_ok!(update_res);
}

fn check_offchain_proposals_num_eq(num: usize) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);
	let stored_props: Option<OffchainSignedProposals> =
		proposals_ref.get::<OffchainSignedProposals>().unwrap();
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
fn handle_unsigned_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 1);
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);
	})
}

#[test]
fn store_signed_proposal_offchain() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
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

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
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

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2);

		assert_ok!(DKGProposalHandler::submit_signed_proposal(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			signed_proposal
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_none(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);
	});
}

#[test]
fn submit_signed_proposal_already_exists() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);

		let signed_proposal = mock_signed_proposal(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposal(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			signed_proposal.clone()
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_none(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);

		// Second submission
		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);

		assert_ok!(DKGProposalHandler::submit_signed_proposal(
			Origin::signed(sr25519::Public::from_raw([1; 32])),
			signed_proposal
		));

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_none(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
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

		assert_ok!(DKGProposalHandler::handle_unsigned_proposal(
			tx_v_2.encode(),
			ProposalAction::Sign(0)
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);

		let mut invalid_sig: Vec<u8> = Vec::new();
		invalid_sig.extend_from_slice(&[0u8, 64]);
		let signed_proposal =
			ProposalType::EVMSigned { data: tx_v_2.encode(), signature: invalid_sig };

		assert_err!(
			DKGProposalHandler::submit_signed_proposal(
				Origin::signed(sr25519::Public::from_raw([1; 32])),
				signed_proposal
			),
			crate::Error::<Test>::ProposalSignatureInvalid
		);

		assert_eq!(
			DKGProposalHandler::unsigned_proposals(0, DKGPayloadKey::EVMProposal(0)).is_some(),
			true
		);
		assert_eq!(
			DKGProposalHandler::signed_proposals(0, DKGPayloadKey::EVMProposal(0)).is_none(),
			true
		);
	});
}

fn make_single_param_propoosal(prop: ProposalType) -> ProposalType {
	let mut buf = vec![];
	// handler address mock
	buf.extend_from_slice(&[1u8; 22]);
	// 32 bytes chain ID
	buf.extend_from_slice(&[
		0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
		0, 1,
	]);
	// 32 bytes nonce
	buf.extend_from_slice(&[
		0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
		0, 1,
	]);
	// 32 bytes parameter
	buf.extend_from_slice(&[4u8; 32]);

	match prop {
		ProposalType::TokenUpdate { .. } => ProposalType::TokenUpdate { data: buf },
		ProposalType::WrappingFeeUpdate { .. } => ProposalType::WrappingFeeUpdate { data: buf },
		_ => panic!("Invalid proposal type"),
	}
}

#[test]
fn force_submit_should_fail_with_invalid_proposal_type() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_err!(
			DKGProposalHandler::force_submit_unsigned_proposal(
				Origin::root(),
				ProposalType::EVMUnsigned { data: tx_v_2.encode() }
			),
			crate::Error::<Test>::ProposalFormatInvalid
		);
	});
}

#[test]
fn force_submit_should_work_with_valid_proposals() {
	execute_test_with(|| {
		// [
		// 	handler_address_0x_prefixed(22),
		// 	chain_id(32),
		// 	nonce(32),
		// 	parameter(32)
		// ]
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_single_param_propoosal(ProposalType::TokenUpdate { data: vec![] })
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(1, DKGPayloadKey::TokenUpdateProposal(1))
				.is_some(),
			true
		);
		assert_ok!(DKGProposalHandler::force_submit_unsigned_proposal(
			Origin::root(),
			make_single_param_propoosal(ProposalType::WrappingFeeUpdate { data: vec![] })
		));
		assert_eq!(
			DKGProposalHandler::unsigned_proposals(1, DKGPayloadKey::WrappingFeeUpdateProposal(1))
				.is_some(),
			true
		);
	});
}
