use super::*;

#[allow(unused)]
use crate::Pallet;
use codec::Encode;
use dkg_runtime_primitives::{
	proposal::{ProposalAction, ProposalHandlerTrait},
	KEY_TYPE,
};
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::RawOrigin;
use hex_literal::hex;
use pallet_dkg_metadata::Pallet as DKGPallet;
use sp_core::{ecdsa, H256, U256};
use sp_io::crypto::{ecdsa_generate, ecdsa_sign_prehashed};
use sp_std::vec::Vec;

use dkg_runtime_primitives::{
	keccak_256, EIP2930Transaction, Proposal, ProposalKind, TransactionAction, TransactionV2,
};

pub fn mock_eth_tx_eip2930(nonce: u8) -> EIP2930Transaction {
	EIP2930Transaction {
		chain_id: 0,
		nonce: U256::from(nonce),
		gas_price: U256::from(0u8),
		gas_limit: U256::from(0u8),
		action: TransactionAction::Create,
		value: U256::from(0u8),
		input: Vec::<u8>::new(),
		access_list: Vec::new(),
		odd_y_parity: false,
		r: H256::from([0u8; 32]),
		s: H256::from([0u8; 32]),
	}
}

pub fn mock_sign_msg(msg: &[u8; 32], pub_key: &ecdsa::Public) -> ecdsa::Signature {
	ecdsa_sign_prehashed(KEY_TYPE, pub_key, msg).expect("Expected a valid signature")
}

pub fn mock_signed_proposal(eth_tx: TransactionV2, pub_key: &ecdsa::Public) -> Proposal {
	let eth_tx_ser = eth_tx.encode();

	let hash = keccak_256(&eth_tx_ser);
	let sig = mock_sign_msg(&hash, pub_key);

	let mut sig_vec: Vec<u8> = Vec::new();
	sig_vec.extend_from_slice(&sig.0);

	return Proposal::Signed {
		kind: ProposalKind::EVM,
		data: eth_tx_ser.clone(),
		signature: sig_vec,
	}
}

benchmarks! {
	submit_signed_proposals {
		let n in 0..(T::MaxSubmissionsPerBatch::get()).into();
		let dkg_pub_key = ecdsa_generate(KEY_TYPE, None);
		DKGPallet::<T>::set_dkg_public_key(dkg_pub_key.encode());
		let caller: T::AccountId = whitelisted_caller();
		let mut signed_proposals = Vec::new();
		for i in 0..n as usize {
			let tx = TransactionV2::EIP2930(mock_eth_tx_eip2930(i as u8));
			let proposal = Pallet::<T>::force_submit_unsigned_proposal(RawOrigin::Root.into(),Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx.encode().clone()
			});
			let signed_prop = mock_signed_proposal(tx, &dkg_pub_key);
			signed_proposals.push(signed_prop)
		}


		assert!(Pallet::<T>::get_unsigned_proposals().len() == n as usize);
	}: _(RawOrigin::Signed(caller), signed_proposals)
	verify {
		assert!(Pallet::<T>::get_unsigned_proposals().len() == 0);
		assert!(Pallet::<T>::signed_proposals_len() == n as usize);
	}

	force_submit_unsigned_proposal {

		let buf = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 38, 87, 136, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].to_vec();

		let proposal = Proposal::Unsigned {
			kind: ProposalKind::TokenAdd,
			data: buf};

	}: _(RawOrigin::Root, proposal)

	verify {
		assert!(Pallet::<T>::get_unsigned_proposals().len() == 1);
	}

}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext_benchmarks(), crate::mock::Test,);
