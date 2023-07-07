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
use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	keccak_256,
	utils::{ecdsa, sr25519},
	AggregatedMisbehaviourReports, AggregatedPublicKeys, MisbehaviourType, ProposalNonce,
	RefreshProposal, KEY_TYPE,
};

use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::RawOrigin;
use sp_io::crypto::{
	ecdsa_generate, ecdsa_sign, ecdsa_sign_prehashed, sr25519_generate, sr25519_sign,
};
use sp_runtime::{key_types::AURA, traits::TrailingZeroInput, Permill};

const MAX_AUTHORITIES: u32 = 100;
const MAX_BLOCKNUMBER: u32 = 100;
const BLOCK_NUMBER: u32 = 2;

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn assert_has_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_has_event(generic_event.into());
}

fn mock_signature(pub_key: ecdsa::Public, dkg_key: ecdsa::Public) -> (Vec<u8>, Vec<u8>) {
	let msg = dkg_key.encode();
	let hash = keccak_256(&msg);
	let signature: ecdsa::Signature = ecdsa_sign_prehashed(KEY_TYPE, &pub_key, &hash).unwrap();
	(msg, signature.encode())
}

fn mock_pub_key() -> ecdsa::Public {
	ecdsa_generate(KEY_TYPE, None)
}

fn mock_misbehaviour_report<T: Config>(
	pub_key: ecdsa::Public,
	offender: T::DKGId,
	misbehaviour_type: MisbehaviourType,
) -> BoundedVec<u8, T::MaxSignatureLength> {
	let session_id: u64 = 1;
	let mut payload = Vec::new();
	payload.extend_from_slice(&match misbehaviour_type {
		MisbehaviourType::Keygen => [0x01],
		MisbehaviourType::Sign => [0x02],
	});
	payload.extend_from_slice(session_id.to_be_bytes().as_ref());
	payload.extend_from_slice(offender.clone().as_ref());
	let hash = keccak_256(&payload);
	let signature = ecdsa_sign_prehashed(KEY_TYPE, &pub_key, &hash).unwrap();

	signature.encode().try_into().unwrap()
}

fn mock_account_id<T: Config>(pub_key: ecdsa::Public) -> T::AccountId {
	pub_key.using_encoded(|entropy| {
		T::AccountId::decode(&mut TrailingZeroInput::new(entropy))
			.expect("infinite input; no invalid input; qed")
	})
}

benchmarks! {

	where_clause {
		where
			T::DKGId: From<ecdsa::Public>,
			T::AccountId : From<sr25519::Public>
	}

	set_signature_threshold {
		// threshold should be less than total number of next authorities
		let mut next_authorities: BoundedVec<_,_> = Default::default();
		for id in 1..= MAX_AUTHORITIES{
			let account_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			next_authorities.try_push(account_id).unwrap();
		}
		NextAuthorities::<T>::put(&next_authorities);
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;


	}: _(RawOrigin::Root, threshold as u16)
	verify {
		assert!(Pallet::<T>::pending_signature_threshold() == threshold as u16 );
	}

	set_keygen_threshold {
		// threshold should be <= total number of next authorities
		// threshold <= PendingSignatureThreshold
		let mut next_authorities: BoundedVec<_,_> = Default::default();
		for id in 1..= MAX_AUTHORITIES{
			let account_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			next_authorities.try_push(account_id).unwrap();
		}
		NextAuthorities::<T>::put(&next_authorities);
		let threshold = u16::try_from(next_authorities.len()).unwrap();
	}: _(RawOrigin::Root, threshold as u16)
	verify {
		assert!(Pallet::<T>::pending_keygen_threshold() == threshold as u16 );
	}

	submit_public_key {
		let n in 4..MAX_AUTHORITIES;
		let dkg_key = ecdsa_generate(KEY_TYPE, None);
		let mut aggregated_public_keys = AggregatedPublicKeys::default();
		let mut current_authorities: BoundedVec<_,_> = Default::default();
		for id in 1..=n {
			let authority_id = mock_pub_key();
			aggregated_public_keys.keys_and_signatures.push(mock_signature(authority_id, dkg_key));
			let account_id = T::DKGId::from(authority_id);
			current_authorities.try_push(account_id).unwrap();
		}
		let threshold = u16::try_from(current_authorities.len() / 2).unwrap() + 1;
		SignatureThreshold::<T>::put(threshold);
		Authorities::<T>::put(&current_authorities);
		let best_authorities = Pallet::<T>::get_best_authorities(threshold as usize, &current_authorities);
		let mut bounded_best_authorities : BoundedVec<_,_> = Default::default();
		for auth in best_authorities {
			bounded_best_authorities.try_push(auth).unwrap();
		}
		BestAuthorities::<T>::put(&bounded_best_authorities);
	}: _(RawOrigin::None, aggregated_public_keys)
	verify {
		let (id, dkg_key) = Pallet::<T>::dkg_public_key();
		assert_last_event::<T>(Event::PublicKeySubmitted{
			compressed_pub_key: dkg_key.clone().into(),
			uncompressed_pub_key: Pallet::<T>::decompress_public_key(dkg_key.clone().into()).unwrap_or_default(),
			}.into());
	}

	submit_next_public_key {
		let n in 3..MAX_AUTHORITIES;
		let dkg_key = ecdsa_generate(KEY_TYPE, None);
		let mut aggregated_public_keys = AggregatedPublicKeys::default();
		let mut next_authorities: BoundedVec<_,_> = Default::default();
		for id in 1..=n {
			let authority_id = mock_pub_key();
			aggregated_public_keys.keys_and_signatures.push(mock_signature(authority_id, dkg_key));
			let account_id = T::DKGId::from(authority_id);
			next_authorities.try_push(account_id).unwrap();
		}
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;
		NextSignatureThreshold::<T>::put(threshold);
		NextAuthorities::<T>::put(&next_authorities);
		let next_best_authorities = Pallet::<T>::get_best_authorities(threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities : BoundedVec<_,_> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<T>::put(&bounded_next_best_authorities);
	}: _(RawOrigin::None, aggregated_public_keys)
	verify {
		let (_ ,next_dkg_key) = Pallet::<T>::next_dkg_public_key().unwrap();
		assert_last_event::<T>(Event::NextPublicKeySubmitted{
			compressed_pub_key: next_dkg_key.clone().into(),
			uncompressed_pub_key: Pallet::<T>::decompress_public_key(next_dkg_key.clone().into()).unwrap_or_default(),
			}.into());
	}

	submit_misbehaviour_reports {
		let n in 3..MAX_AUTHORITIES;
		let offender: T::DKGId = T::DKGId::from(ecdsa_generate(KEY_TYPE, None));
		let mut next_authorities: BoundedVec<_,_> = Default::default();
		let mut reporters: Vec<T::DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let session_id = 1;
		let misbehaviour_type = MisbehaviourType::Keygen;
		for id in 1..=n{
			let authority_id = mock_pub_key();
			let sig = mock_misbehaviour_report::<T>(authority_id, offender.clone(), misbehaviour_type);
			signatures.try_push(sig).unwrap();
			let dkg_id = T::DKGId::from(authority_id);
			reporters.push(dkg_id.clone());
			next_authorities.try_push(dkg_id).unwrap();
		}
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;
		NextSignatureThreshold::<T>::put(threshold);
		NextAuthorities::<T>::put(&next_authorities);
		let aggregated_misbehaviour_reports= AggregatedMisbehaviourReports {
													misbehaviour_type,
													session_id,
													offender : offender.clone(),
													reporters:reporters.clone().try_into().unwrap(),
													signatures : signatures.try_into().unwrap(),
												};
		let caller = T::AccountId::from(sr25519::Public::from_raw([1u8; 32]));
	}: _(RawOrigin::None, aggregated_misbehaviour_reports)
	verify {
		assert_last_event::<T>(Event::MisbehaviourReportsSubmitted{
			misbehaviour_type,
			reporters: reporters.clone().try_into().unwrap(),
			offender,
			}.into());
	}

	unjail {
		for id in 1..MAX_AUTHORITIES{
			let dkg_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			let account_id = T::AccountId::from(sr25519::Public::from_raw([id as u8; 32]));
			let block_number: T::BlockNumber = id.into();
			AccountToAuthority::<T>::insert(&account_id, dkg_id.clone());
			JailedKeygenAuthorities::<T>::insert(dkg_id.clone(), block_number);
			JailedSigningAuthorities::<T>::insert(dkg_id.clone(), block_number);
		}
		let caller = T::AccountId::from(sr25519::Public::from_raw([1u8; 32]));
		let offender = T::DKGId::from(ecdsa::Public::from_raw([1u8; 33]));
		let key_gen_sentence = T::KeygenJailSentence::get();
		let block_number = key_gen_sentence + T::BlockNumber::from(BLOCK_NUMBER);
		frame_system::Pallet::<T>::set_block_number(block_number.into());
	}: _(RawOrigin::Signed(caller))
	verify {
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone()) == false);
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone()) == false);
	}

	force_unjail_signing {
		for id in 1..MAX_AUTHORITIES{
			let dkg_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			let block_number: T::BlockNumber = id.into();
			JailedSigningAuthorities::<T>::insert(dkg_id.clone(), block_number);
		}
		let offender = T::DKGId::from(ecdsa::Public::from_raw([1u8; 33]));
	}: _(RawOrigin::Root, offender.clone())
	verify {
		assert!(JailedSigningAuthorities::<T>::contains_key(offender.clone()) == false);
	}

	force_unjail_keygen {
		for id in 1..MAX_AUTHORITIES{
			let dkg_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			let block_number: T::BlockNumber = id.into();
			JailedKeygenAuthorities::<T>::insert(dkg_id.clone(), block_number);
		}
		let offender = T::DKGId::from(ecdsa::Public::from_raw([1u8; 33]));
	}: _(RawOrigin::Root, offender.clone())
	verify {
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone()) == false);
	}
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(vec![1, 2, 3, 4]), crate::mock::Test);
