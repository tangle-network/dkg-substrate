use crate::mock::*;
use codec::{Decode, Encode};
use frame_support::assert_ok;
use sp_runtime::offchain::storage::{StorageRetrievalError, StorageValueRef};
use sp_std::vec::Vec;

use super::mock::DKGProposalHandler;
use dkg_runtime_primitives::{
	EIP2930Transaction, OffchainSignedProposals, ProposalAction, ProposalHandlerTrait,
	ProposalType, TransactionAction, TransactionV2, OFFCHAIN_SIGNED_PROPOSALS, U256,
};
use sp_core::H256;
use sp_runtime::offchain::storage::MutateStorageError;

// *** Utility ***

fn add_proposal_to_offchain_storage(prop: ProposalType) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

	let update_res: Result<Vec<u8>, MutateStorageError<Vec<u8>, ()>> =
		proposals_ref.mutate(|val: Result<Option<Vec<u8>>, StorageRetrievalError>| match val {
			Ok(Some(ser_props)) =>
				if ser_props.len() > 0 {
					let mut prop_wrapper =
						OffchainSignedProposals::decode(&mut &ser_props[..]).unwrap();
					prop_wrapper.proposals.push_back(prop);
					Ok(prop_wrapper.encode())
				} else {
					let mut prop_wrapper = OffchainSignedProposals::default();
					prop_wrapper.proposals.push_back(prop);
					Ok(prop_wrapper.encode())
				},
			_ => {
				let mut prop_wrapper = OffchainSignedProposals::default();
				prop_wrapper.proposals.push_back(prop);
				Ok(prop_wrapper.encode())
			},
		});

	assert_ok!(update_res);
}

fn check_offchain_proposals_num_eq(num: usize) {
	let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);
	let stored_props: Option<Vec<u8>> = proposals_ref.get().unwrap();
	assert_eq!(stored_props.is_some(), true);

	let prop_wrapper = OffchainSignedProposals::decode(&mut &stored_props.unwrap()[..]);
	assert_ok!(&prop_wrapper);
	assert_eq!(prop_wrapper.unwrap().proposals.len(), num);
}

// *** Tests ***

#[test]
fn handle_empty_proposal() {
	execute_test_with(|| {
		let prop: Vec<u8> = Vec::new();

		assert_ok!(DKGProposalHandler::handle_proposal(prop, ProposalAction::Sign(0)));

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 0);
	});
}

#[test]
fn handle_unsigned_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));

		assert_eq!(DKGProposalHandler::get_unsigned_proposals().len(), 1);
		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);
	})
}

#[test]
fn store_signed_proposal_offchain() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		let unsigned_prop = DKGProposalHandler::unsigned_proposals(0, 0);
		assert_eq!(unsigned_prop.is_some(), true);

		add_proposal_to_offchain_storage(unsigned_prop.unwrap());

		check_offchain_proposals_num_eq(1);
	})
}

#[test]
fn submit_signed_proposal_onchain_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		let unsigned_prop = DKGProposalHandler::unsigned_proposals(0, 0);
		assert_eq!(unsigned_prop.is_some(), true);

		add_proposal_to_offchain_storage(unsigned_prop.unwrap());

		assert_ok!(DKGProposalHandler::submit_signed_proposal_onchain(0));

		check_offchain_proposals_num_eq(0);
	});
}

#[test]
fn submit_signed_proposal_success() {
	execute_test_with(|| {
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);

		let signed_proposal = mock_signed_proposal(tx_v_2);

		assert_ok!(DKGProposalHandler::submit_signed_proposal(Origin::root(), signed_proposal));

		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_none(), true);
		assert_eq!(DKGProposalHandler::signed_proposals(0, 0).is_some(), true);
	});
}

#[test]
fn submit_signed_proposal_fail_already_exists() {
	execute_test_with(|| {
		// First submission
		let tx_v_2 = TransactionV2::EIP2930(mock_eth_tx_eip2930(0));

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);

		let signed_proposal = mock_signed_proposal(tx_v_2.clone());

		assert_ok!(DKGProposalHandler::submit_signed_proposal(
			Origin::root(),
			signed_proposal.clone()
		));

		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_none(), true);
		assert_eq!(DKGProposalHandler::signed_proposals(0, 0).is_some(), true);

		// Second submission
		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);

		assert_ok!(DKGProposalHandler::submit_signed_proposal(Origin::root(), signed_proposal));

		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_none(), true);
		assert_eq!(DKGProposalHandler::signed_proposals(0, 0).is_some(), true);
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
		let tx_v_2 = TransactionV2::EIP2930(tx_eip2930);

		assert_ok!(DKGProposalHandler::handle_proposal(tx_v_2.encode(), ProposalAction::Sign(0)));
		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);

		let mut invalid_sig: Vec<u8> = Vec::new();
		invalid_sig.extend_from_slice(&[0u8, 64]);
		let signed_proposal =
			ProposalType::EVMSigned { data: tx_v_2.encode(), signature: invalid_sig };

		assert_eq!(
			DKGProposalHandler::submit_signed_proposal(Origin::root(), signed_proposal).is_err(),
			true
		);

		assert_eq!(DKGProposalHandler::unsigned_proposals(0, 0).is_some(), true);
		assert_eq!(DKGProposalHandler::signed_proposals(0, 0).is_none(), true);
	});
}
