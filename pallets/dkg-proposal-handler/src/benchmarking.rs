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
use super::*;

#[allow(unused)]
use crate::Pallet;
use codec::Encode;
use dkg_runtime_primitives::KEY_TYPE;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::RawOrigin;
use pallet_dkg_metadata::Pallet as DKGPallet;
use sp_core::{ecdsa, H256, U256};
use sp_io::crypto::{ecdsa_generate, ecdsa_sign_prehashed};
use sp_std::vec::Vec;

use dkg_runtime_primitives::{keccak_256, EIP2930Transaction, TransactionAction, TransactionV2};
use webb_proposals::{Proposal, ProposalKind};

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

pub fn mock_signed_proposal<T: Config>(
	eth_tx: TransactionV2,
	pub_key: &ecdsa::Public,
) -> Proposal<T::MaxProposalLength> {
	let eth_tx_ser = eth_tx.encode();

	let hash = keccak_256(&eth_tx_ser);
	let sig = mock_sign_msg(&hash, pub_key);

	let mut sig_vec: Vec<u8> = Vec::new();
	sig_vec.extend_from_slice(&sig.0);

	Proposal::Signed {
		kind: ProposalKind::EVM,
		data: eth_tx_ser.try_into().unwrap(),
		signature: sig_vec.try_into().unwrap(),
	}
}

benchmarks! {
	submit_signed_proposals {
		let n in 0..(T::MaxSubmissionsPerBatch::get()).into();
		let dkg_pub_key = ecdsa_generate(KEY_TYPE, None);
		let bounded_dkg_pub_key : BoundedVec<_,_> = dkg_pub_key.encode().try_into().unwrap();
		DKGPallet::<T>::set_dkg_public_key(bounded_dkg_pub_key);
		let caller: T::AccountId = whitelisted_caller();
		let mut signed_proposals = Vec::new();
		for i in 0..n as usize {
			let tx = TransactionV2::EIP2930(mock_eth_tx_eip2930(i as u8));
			let proposal = Pallet::<T>::force_submit_unsigned_proposal(RawOrigin::Root.into(),Proposal::Unsigned {
				kind: ProposalKind::EVM,
				data: tx.encode().clone().try_into().unwrap()
			});
			let signed_prop = mock_signed_proposal::<T>(tx, &dkg_pub_key);
			signed_proposals.push(signed_prop)
		}
	}: _(RawOrigin::Signed(caller), signed_proposals)
	verify {
		assert!(Pallet::<T>::get_unsigned_proposal_batches().is_empty());
		assert!(Pallet::<T>::signed_proposals_len() == n as usize);
	}

	force_submit_unsigned_proposal {
		let buf = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 38, 87, 136, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].to_vec();

		let proposal = Proposal::Unsigned {
			kind: ProposalKind::TokenAdd,
			data: buf.try_into().unwrap()
		};
	}: _(RawOrigin::Root, proposal)
	verify {
		assert!(Pallet::<T>::get_unsigned_proposal_batches().len() == 1);
	}

	force_remove_unsigned_proposal {
		let buf = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 38, 87, 136, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].to_vec();
		let proposal = Proposal::Unsigned {
			kind: ProposalKind::TokenAdd,
			data: buf.try_into().unwrap()
		};
		Pallet::<T>::force_submit_unsigned_proposal(RawOrigin::Root.into(), proposal.clone()).unwrap();
		assert!(Pallet::<T>::get_unsigned_proposals().len() == 1);
		let prop_identifier = decode_proposal_identifier(&proposal).unwrap();
	}: _(RawOrigin::Root, prop_identifier.typed_chain_id, prop_identifier.key)
	verify {
		assert!(Pallet::<T>::get_unsigned_proposals().len() == 0);
	}

}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext_benchmarks(), crate::mock::Test,);
