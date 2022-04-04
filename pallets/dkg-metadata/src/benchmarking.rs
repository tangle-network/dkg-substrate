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
//! Benchmarking for dkg-metadata

use super::*;

#[allow(unused)]
use crate::Pallet;
use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	keccak_256,
	utils::{ecdsa, sr25519},
	AggregatedMisbehaviourReports, AggregatedPublicKeys, MisbehaviourType, ProposalNonce,
	RefreshProposal, RefreshProposalSigned, KEY_TYPE,
};

use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::RawOrigin;
use sp_io::crypto::{ecdsa_generate, ecdsa_sign_prehashed, sr25519_generate, sr25519_sign};
use sp_runtime::{key_types::AURA, traits::TrailingZeroInput, Permill};

const MAX_AUTHORITIES: u32 = 20;
const MAX_BLOCKNUMBER: u32 = 100;
const BLOCK_NUMBER: u32 = 2;

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn assert_has_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_has_event(generic_event.into());
}

fn mock_signature(pub_key: sr25519::Public, dkg_key: ecdsa::Public) -> (Vec<u8>, Vec<u8>) {
	let msg = dkg_key.encode();
	let signature: sr25519::Signature = sr25519_sign(AURA, &pub_key, &msg).unwrap();
	(msg, signature.encode())
}

fn mock_pub_key() -> sr25519::Public {
	sr25519_generate(AURA, None)
}

fn mock_misbehaviour_report<T: Config>(
	pub_key: sr25519::Public,
	offender: T::DKGId,
	misbehaviour_type: MisbehaviourType,
) -> Vec<u8> {
	let round_id: u64 = 1;
	let mut payload = Vec::new();
	payload.extend_from_slice(&match misbehaviour_type {
		MisbehaviourType::Keygen => [0x01],
		MisbehaviourType::Sign => [0x02],
	});
	payload.extend_from_slice(round_id.to_be_bytes().as_ref());
	payload.extend_from_slice(offender.clone().as_ref());

	let signature = sr25519_sign(AURA, &pub_key, &payload).unwrap();

	signature.encode()
}

fn mock_account_id<T: Config>(pub_key: sr25519::Public) -> T::AccountId {
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
		let mut next_authorities:Vec<T::DKGId> = Vec::new();
		for id in 1..=MAX_AUTHORITIES{
			let account_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			next_authorities.push(account_id);
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
		let mut next_authorities:Vec<T::DKGId> = Vec::new();
		for id in 1..=MAX_AUTHORITIES{
			let account_id = T::DKGId::from(ecdsa::Public::from_raw([id as u8; 33]));
			next_authorities.push(account_id);
		}
		NextAuthorities::<T>::put(&next_authorities);
		let threshold = u16::try_from(next_authorities.len()).unwrap();
	}: _(RawOrigin::Root, threshold as u16)
	verify {
		assert!(Pallet::<T>::pending_keygen_threshold() == threshold as u16 );
	}

	set_refresh_delay {
		// new refresh delay should be <= 100
		let n in 1..100;
	}: _(RawOrigin::Root, n as u8)
	verify {
		assert!(Pallet::<T>::refresh_delay() == Permill::from_percent(n as u32));
	}

	manual_increment_nonce {
		let refresh_nounce = Pallet::<T>::refresh_nonce();
	}: _(RawOrigin::Root)
	verify {
		assert!(Pallet::<T>::refresh_nonce() ==  refresh_nounce+1);
	}

	manual_refresh {
		let current_dkg = ecdsa_generate(KEY_TYPE,None);
		let next_dkg = ecdsa_generate(KEY_TYPE,None);
		DKGPublicKey::<T>::put((0,current_dkg.encode()));
		NextDKGPublicKey::<T>::put((1,next_dkg.encode()));
	}: _(RawOrigin::Root)
	verify {
		assert!(Pallet::<T>::should_manual_refresh() ==  true);
	}

	submit_public_key {
		let n in 3..MAX_AUTHORITIES;
		let dkg_key = ecdsa_generate(KEY_TYPE,None);
		let mut aggregated_public_keys = AggregatedPublicKeys::default();
		let mut current_authorities:Vec<T::AccountId> = Vec::new();
		for id in 1..=n {
			let authority_id = mock_pub_key();
			aggregated_public_keys.keys_and_signatures.push(mock_signature(authority_id,dkg_key.clone()));
			current_authorities.push(mock_accoun_id::<T>(authority_id));
		}
		let threshold = u16::try_from(current_authorities.len() / 2).unwrap() + 1;
		SignatureThreshold::<T>::put(threshold);
		CurrentAuthoritiesAccounts::<T>::put(&current_authorities);
		let caller = current_authorities[0].clone();
	}: _(RawOrigin::Signed(caller), aggregated_public_keys)
	verify {
		let (id ,dkg_key) = Pallet::<T>::dkg_public_key();
		assert_last_event::<T>(Event::PublicKeySubmitted{
			compressed_pub_key: dkg_key.clone(),
			uncompressed_pub_key: Pallet::<T>::decompress_public_key(dkg_key.clone()).unwrap_or_default(),
			}.into());
	}

	submit_next_public_key {
		let n in 3..MAX_AUTHORITIES;
		let dkg_key = ecdsa_generate(KEY_TYPE,None);
		let mut aggregated_public_keys = AggregatedPublicKeys::default();
		let mut next_authorities:Vec<T::AccountId> = Vec::new();
		for id in 1..=n {
			let authority_id = mock_pub_key();
			aggregated_public_keys.keys_and_signatures.push(mock_signature(authority_id,dkg_key.clone()));
			next_authorities.push(mock_accoun_id::<T>(authority_id));
		}
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;
		NextSignatureThreshold::<T>::put(threshold);
		NextAuthoritiesAccounts::<T>::put(&next_authorities);
		let caller = next_authorities[0].clone();
	}: _(RawOrigin::Signed(caller), aggregated_public_keys)
	verify {
		let (_ ,next_dkg_key) = Pallet::<T>::next_dkg_public_key().unwrap();
		assert_last_event::<T>(Event::NextPublicKeySubmitted{
			compressed_pub_key: next_dkg_key.clone(),
			uncompressed_pub_key: Pallet::<T>::decompress_public_key(next_dkg_key.clone()).unwrap_or_default(),
			}.into());
	}

	submit_public_key_signature {
		let current_dkg = ecdsa_generate(KEY_TYPE,None);
		let next_dkg =  ecdsa_generate(KEY_TYPE,None);
		DKGPublicKey::<T>::put((0,current_dkg.encode()));
		NextDKGPublicKey::<T>::put((1,next_dkg.encode()));
		let uncompressed_pub_key = Pallet::<T>::decompress_public_key(next_dkg.encode()).unwrap();
		let refresh_nounce = Pallet::<T>::refresh_nonce();
		let refresh_proposal = RefreshProposal {
								nonce: ProposalNonce::from(0),
								pub_key: uncompressed_pub_key,
								};
		let hash = keccak_256(&refresh_proposal.encode());
		let signature = ecdsa_sign_prehashed(KEY_TYPE, &current_dkg, &hash).expect("Expected a valid signature");
		let signed_proposal = RefreshProposalSigned { nonce: ProposalNonce::from(0),
													 signature: signature.encode()
													};
		ShouldManualRefresh::<T>::put(true);
		let all_accounts = Pallet::<T>::current_authorities_accounts();
		let caller:T::AccountId = all_accounts[0].clone();
	}: _(RawOrigin::Signed(caller), signed_proposal)
	verify {
		assert!(Pallet::<T>::should_manual_refresh()== false);
		assert_has_event::<T>(Event::NextPublicKeySignatureSubmitted{
			pub_key_sig: signature.encode(),
			}.into());
	}

	submit_misbehaviour_reports {
		let n in 3..MAX_AUTHORITIES;
		let offender:T::DKGId = T::DKGId::from(ecdsa_generate(KEY_TYPE,None));
		let mut next_authorities:Vec<T::AccountId> = Vec::new();
		let mut reporters:Vec<sr25519::Public> = Vec::new();
		let mut signatures: Vec<Vec<u8>> = Vec::new();
		let round_id = 1;
		let misbehaviour_type = MisbehaviourType::Keygen;
		for id in 1..=n{
			let authority_id = mock_pub_key();
			let sig = mock_misbehaviour_report::<T>(authority_id,offender.clone(),misbehaviour_type);
			signatures.push(sig);
			reporters.push(authority_id);
			next_authorities.push(mock_accoun_id::<T>(authority_id));
		}
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;
		NextSignatureThreshold::<T>::put(threshold);
		NextAuthoritiesAccounts::<T>::put(&next_authorities);
		let aggregated_misbehaviour_reports= AggregatedMisbehaviourReports {
													misbehaviour_type,
													round_id,
													offender,
													reporters:reporters.clone(),
													signatures,
												};
		let caller = next_authorities[0].clone();
	}: _(RawOrigin::Signed(caller), aggregated_misbehaviour_reports)
	verify {
		assert_last_event::<T>(Event::MisbehaviourReportsSubmitted{
			misbehaviour_type,
			reporters: reporters.clone(),
			}.into());
	}

	unjail {
		let offender = T::DKGId::from(ecdsa_generate(KEY_TYPE,None));
		let account_id = T::AccountId::from(mock_pub_key());
		let block_number: T::BlockNumber = BLOCK_NUMBER.into();
		AccountToAuthority::<T>::insert(&account_id,offender.clone());
		JailedKeygenAuthorities::<T>::insert(offender.clone(),block_number);
		JailedSigningAuthorities::<T>::insert(offender.clone(),block_number);
		let key_gen_sentence = T::KeygenJailSentence::get();
		let block_number = key_gen_sentence + T::BlockNumber::from(BLOCK_NUMBER) + T::BlockNumber::from(1u32);
		frame_system::Pallet::<T>::set_block_number(block_number.into());
	}: _(RawOrigin::Signed(account_id))
	verify {
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone())== false);
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone())== false);
	}

	force_unjail_signing {
		let offender = T::DKGId::from(ecdsa_generate(KEY_TYPE,None));
		let block_number : T::BlockNumber = BLOCK_NUMBER.into();
		JailedSigningAuthorities::<T>::insert(offender.clone(),block_number);
	}: _(RawOrigin::Root,offender.clone())
	verify {
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone())== false);
	}

	force_unjail_keygen {
		let offender = T::DKGId::from(ecdsa_generate(KEY_TYPE,None));
		let block_number: T::BlockNumber = BLOCK_NUMBER.into();
		JailedKeygenAuthorities::<T>::insert(offender.clone(),block_number);
	}: _(RawOrigin::Root,offender.clone())
	verify {
		assert!(JailedKeygenAuthorities::<T>::contains_key(offender.clone())== false);
	}
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(vec![1, 2, 3, 4]), crate::mock::Test);
