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
#![allow(clippy::unwrap_used)]
use std::vec;

use crate::{
	mock::*, AggregatedMisbehaviourReports, AuthorityReputations, Config, Error, Event,
	JailedKeygenAuthorities, NextAuthorities, NextBestAuthorities, NextKeygenThreshold,
	NextSignatureThreshold,
};
use codec::Encode;
use dkg_runtime_primitives::{keccak_256, utils::ecdsa, MisbehaviourType, KEY_TYPE};
use frame_support::{assert_noop, assert_ok, traits::Hooks, weights::Weight, BoundedVec};
use sp_core::ByteArray;
use sp_io::crypto::{ecdsa_generate, ecdsa_sign_prehashed};
use sp_runtime::traits::Bounded;

fn init_block(block: u64) {
	System::set_block_number(block);
	Session::on_initialize(block);
	DKGMetadata::on_initialize(block);
	DKGMetadata::on_idle(block, Weight::max_value());
	DKGMetadata::on_finalize(block);
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

fn mock_pub_key() -> ecdsa::Public {
	ecdsa_generate(KEY_TYPE, None)
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn assert_has_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_has_event(generic_event.into());
}

#[test]
fn genesis_session_initializes_authorities() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		let authorities = DKGMetadata::authorities();

		assert!(authorities.len() == 2);
		assert_eq!(want[0], authorities[0]);
		assert_eq!(want[1], authorities[1]);

		assert!(DKGMetadata::authority_set_id() == 0);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[0], next_authorities[0]);
		assert_eq!(want[1], next_authorities[1]);
	});
}

#[test]
fn session_change_updates_next_authorities() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[2], next_authorities[0]);
		assert_eq!(want[3], next_authorities[1]);

		init_block(2);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[2], next_authorities[0]);
		assert_eq!(want[3], next_authorities[1]);
	});
}

#[test]
fn authority_set_at_genesis() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 0u64);
		assert_eq!(vs.authorities[0], want[0]);
		assert_eq!(vs.authorities[1], want[1]);
	});
}

#[test]
fn next_authority_set_updates_work() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);

		let vs = DKGMetadata::next_authority_set();

		assert_eq!(vs.id, 2u64);
		assert_eq!(want[2], vs.authorities[0]);
		assert_eq!(want[3], vs.authorities[1]);

		init_block(2);

		let vs = DKGMetadata::next_authority_set();

		assert_eq!(vs.id, 3u64);
		assert_eq!(want[2], vs.authorities[0]);
		assert_eq!(want[3], vs.authorities[1]);
	});
}

// a test to ensure that when we set `ShouldTriggerEmergencyKeygen` to true, it will emit
// an `EmertgencyKeygenTriggered` RuntimeEvent and next block it will be reset back to false.
#[test]
fn trigger_emergency_keygen_works() {
	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		System::set_block_number(1);
		DKGMetadata::on_initialize(1);
		assert_eq!(DKGMetadata::should_execute_new_keygen(), (false, false));
		assert_ok!(DKGMetadata::trigger_emergency_keygen(RuntimeOrigin::root()));
		assert_eq!(DKGMetadata::should_execute_new_keygen(), (true, true));
		System::set_block_number(2);
		DKGMetadata::on_initialize(2);
		// it should be reset to false after the block is initialized
		assert_eq!(DKGMetadata::should_execute_new_keygen(), (false, false));
	});
}

#[test]
fn refresh_nonce_should_increment_by_one() {
	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);
		let refresh_nonce = DKGMetadata::refresh_nonce();
		assert_eq!(refresh_nonce, 0);
		let input: BoundedVec<_, _> = mock_dkg_id(1).to_raw_vec().try_into().unwrap();
		crate::pallet::NextDKGPublicKey::<Test>::put((1, input));
		let next_pub_key_signature: BoundedVec<_, _> = vec![1u8; 64].try_into().unwrap();
		crate::pallet::NextPublicKeySignature::<Test>::put(next_pub_key_signature);
		init_block(2);
		let refresh_nonce = DKGMetadata::refresh_nonce();
		assert_eq!(refresh_nonce, 1);

		let input: BoundedVec<_, _> = mock_dkg_id(2).to_raw_vec().try_into().unwrap();
		crate::pallet::NextDKGPublicKey::<Test>::put((2, input));
		let next_pub_key_signature: BoundedVec<_, _> = vec![2u8; 64].try_into().unwrap();
		crate::pallet::NextPublicKeySignature::<Test>::put(next_pub_key_signature);
		init_block(3);
		let refresh_nonce = DKGMetadata::refresh_nonce();
		assert_eq!(refresh_nonce, 2);
	});
}

#[test]
fn misbehaviour_reports_submission_rejects_if_offender_not_authority() {
	new_test_ext(vec![1, 2, 3, 4, 5]).execute_with(|| {
		// lets use a random offender
		let offender: DKGId = DKGId::from(ecdsa_generate(KEY_TYPE, None));
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut reporters: Vec<DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let session_id = 1;
		let misbehaviour_type = MisbehaviourType::Keygen;
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let sig =
				mock_misbehaviour_report::<Test>(authority_id, offender.clone(), misbehaviour_type);
			signatures.try_push(sig).unwrap();
			let dkg_id = DKGId::from(authority_id);
			reporters.push(dkg_id.clone());
			next_authorities.try_push(dkg_id).unwrap();
		}
		let threshold = u16::try_from(next_authorities.len() / 2).unwrap() + 1;
		NextSignatureThreshold::<Test>::put(threshold);
		NextAuthorities::<Test>::put(&next_authorities);
		let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
			DKGId,
			MaxSignatureLength,
			MaxReporters,
		> = AggregatedMisbehaviourReports {
			misbehaviour_type,
			session_id,
			offender: offender.clone(),
			reporters: reporters.clone().try_into().unwrap(),
			signatures: signatures.try_into().unwrap(),
		};

		assert_noop!(
			DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports
			),
			Error::<Test>::OffenderNotAuthority
		);
	});
}

#[test]
fn keygen_misbehaviour_reports_work() {
	new_test_ext(vec![1, 2, 3, 4, 5]).execute_with(|| {
		let session_id = 1;

		// prep the next authorities
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut next_authorities_raw: Vec<_> = Default::default();
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let dkg_id = DKGId::from(authority_id);
			next_authorities_raw.push(authority_id);
			next_authorities.try_push(dkg_id).unwrap();
		}

		// 1 is the misbehaving party
		let offender: DKGId = next_authorities.first().unwrap().clone();
		// lets give the offender some reputation
		AuthorityReputations::<Test>::insert(offender.clone(), 100);

		let mut reporters: Vec<DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let misbehaviour_type = MisbehaviourType::Keygen;

		// everyone except 1 reports the misbehaviour
		for authority_id in next_authorities_raw.iter() {
			let dkg_id = DKGId::from(authority_id.clone());

			if dkg_id == offender {
				continue
			}

			let sig = mock_misbehaviour_report::<Test>(
				authority_id.clone(),
				offender.clone(),
				misbehaviour_type,
			);
			signatures.try_push(sig).unwrap();
			reporters.push(dkg_id.clone());
		}

		// setup the new threshold so our signature is accepted
		let keygen_threshold = 5;
		let signature_threshold = 3;
		NextKeygenThreshold::<Test>::put(keygen_threshold);
		NextSignatureThreshold::<Test>::put(signature_threshold);
		NextAuthorities::<Test>::put(&next_authorities);
		let next_best_authorities =
			DKGMetadata::get_best_authorities(keygen_threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities: BoundedVec<_, _> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<Test>::put(&bounded_next_best_authorities);

		let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
			DKGId,
			MaxSignatureLength,
			MaxReporters,
		> = AggregatedMisbehaviourReports {
			misbehaviour_type,
			session_id,
			offender: offender.clone(),
			reporters: reporters.clone().try_into().unwrap(),
			signatures: signatures.try_into().unwrap(),
		};

		assert_ok!(DKGMetadata::submit_misbehaviour_reports(
			RuntimeOrigin::none(),
			aggregated_misbehaviour_reports.clone()
		));

		// succcessful submission event should be present
		assert_last_event::<Test>(
			Event::MisbehaviourReportsSubmitted {
				misbehaviour_type,
				reporters: reporters.clone().try_into().unwrap(),
				offender: offender.clone(),
			}
			.into(),
		);

		// succcessful submission event should be present
		assert_has_event::<Test>(
			Event::AuthorityJailed { misbehaviour_type, authority: offender.clone() }.into(),
		);

		// lets check the storage is as expected

		// jailed authorities should contain the offender
		assert_eq!(JailedKeygenAuthorities::<Test>::get(offender.clone()), session_id);

		// offender reputation is dinged by decay percentage
		assert_eq!(AuthorityReputations::<Test>::get(offender.clone()), 50);

		// next best authorities should contain everyone except the offender
		let mut current_next_best_authorities = NextBestAuthorities::<Test>::get();
		assert_eq!(current_next_best_authorities.len(), 4);
		current_next_best_authorities.retain(|x| x.1 == offender);
		assert_eq!(current_next_best_authorities.len(), 0);

		// the threshold should have reduced correctly
		assert_eq!(NextKeygenThreshold::<Test>::get(), keygen_threshold - 1);
		assert_eq!(NextSignatureThreshold::<Test>::get(), signature_threshold);

		// sanity check , cannot report the same party twice
		assert_noop!(
			DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports
			),
			Error::<Test>::AlreadyJailed
		);
	});
}

// Test that a keygen misbehaviour report will reduce signing threshold if needed
// our goal is to ensure we never end up in a t = n situation
#[test]
fn keygen_misbehaviour_reports_reduce_signature_threshold_if_needed() {
	new_test_ext(vec![1, 2, 3, 4, 5]).execute_with(|| {
		let session_id = 1;

		// setup the new threshold so our signature is accepted
		let keygen_threshold = 5;
		let signature_threshold = 3;
		NextKeygenThreshold::<Test>::put(keygen_threshold);
		NextSignatureThreshold::<Test>::put(signature_threshold);

		// prep the next authorities
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut next_authorities_raw: Vec<_> = Default::default();
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let dkg_id = DKGId::from(authority_id);
			next_authorities_raw.push(authority_id);
			next_authorities.try_push(dkg_id).unwrap();
		}

		// load them onchain
		NextAuthorities::<Test>::put(&next_authorities);
		let next_best_authorities =
			DKGMetadata::get_best_authorities(keygen_threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities: BoundedVec<_, _> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<Test>::put(&bounded_next_best_authorities);

		let mut offenders = vec![];

		// lets jail two authorities one after other
		for i in 0..2 {
			let offender: DKGId = next_authorities[i].clone();
			offenders.push(offender.clone());
			// lets give the offender some reputation
			AuthorityReputations::<Test>::insert(offender.clone(), 100);

			let mut reporters: Vec<DKGId> = Vec::new();
			let mut signatures: BoundedVec<_, _> = Default::default();
			let misbehaviour_type = MisbehaviourType::Keygen;

			// everyone except 1 reports the misbehaviour
			for authority_id in next_authorities_raw.iter() {
				let dkg_id = DKGId::from(authority_id.clone());

				if dkg_id == offender {
					continue
				}

				let sig = mock_misbehaviour_report::<Test>(
					authority_id.clone(),
					offender.clone(),
					misbehaviour_type,
				);
				signatures.try_push(sig).unwrap();
				reporters.push(dkg_id.clone());
			}

			let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
				DKGId,
				MaxSignatureLength,
				MaxReporters,
			> = AggregatedMisbehaviourReports {
				misbehaviour_type,
				session_id,
				offender: offender.clone(),
				reporters: reporters.clone().try_into().unwrap(),
				signatures: signatures.try_into().unwrap(),
			};

			assert_ok!(DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports
			));

			// succcessful submission event should be present
			assert_last_event::<Test>(
				Event::MisbehaviourReportsSubmitted {
					misbehaviour_type,
					reporters: reporters.clone().try_into().unwrap(),
					offender: offender.clone(),
				}
				.into(),
			);

			// succcessful submission event should be present
			assert_has_event::<Test>(
				Event::AuthorityJailed { misbehaviour_type, authority: offender.clone() }.into(),
			);
		}

		// lets check the storage is as expected

		// jailed authorities should contain both the offenders
		for offender in offenders.iter() {
			assert_eq!(JailedKeygenAuthorities::<Test>::get(offender), 1);
		}

		// next best authorities should contain everyone except the offenders
		let mut current_next_best_authorities = NextBestAuthorities::<Test>::get();
		assert_eq!(current_next_best_authorities.len() as u16, keygen_threshold - 2);
		current_next_best_authorities.retain(|x| offenders.contains(&x.1));
		assert_eq!(current_next_best_authorities.len(), 0);

		// the threshold should have reduced correctly
		assert_eq!(NextKeygenThreshold::<Test>::get(), keygen_threshold - 2);

		// should correctly drop the signature threshold to ensure we do not end up in t = n
		assert_eq!(NextSignatureThreshold::<Test>::get(), signature_threshold - 1);
	});
}

// Test that the signature thresholds are as expected, the misbehaviour report should only be
// accepted if the signature contains t+1 signers
#[test]
fn keygen_misbehaviour_reports_fail_if_not_threshold_plus_1() {
	new_test_ext(vec![1, 2, 3, 4, 5]).execute_with(|| {
		let session_id = 1;
		// setup the new threshold so our signature is accepted
		let keygen_threshold = 5;
		let signature_threshold = 3;
		NextKeygenThreshold::<Test>::put(keygen_threshold);
		NextSignatureThreshold::<Test>::put(signature_threshold);

		// prep the next authorities
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut next_authorities_raw: Vec<_> = Default::default();
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let dkg_id = DKGId::from(authority_id);
			next_authorities_raw.push(authority_id);
			next_authorities.try_push(dkg_id).unwrap();
		}
		NextAuthorities::<Test>::put(&next_authorities);
		let next_best_authorities =
			DKGMetadata::get_best_authorities(keygen_threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities: BoundedVec<_, _> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<Test>::put(&bounded_next_best_authorities);

		// 1 is the misbehaving party
		let offender: DKGId = next_authorities.first().unwrap().clone();
		// lets give the offender some reputation
		AuthorityReputations::<Test>::insert(offender.clone(), 100);

		let mut reporters: Vec<DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let misbehaviour_type = MisbehaviourType::Keygen;

		// we ideally require threshold + 1 signatures for misbehaviour to be accepted
		// lets sign with exactly threshold
		for authority_id in next_authorities_raw.iter() {
			let dkg_id = DKGId::from(authority_id.clone());

			if dkg_id == offender {
				continue
			}

			let sig = mock_misbehaviour_report::<Test>(
				authority_id.clone(),
				offender.clone(),
				misbehaviour_type,
			);
			signatures.try_push(sig).unwrap();
			reporters.push(dkg_id.clone());

			if reporters.len() == signature_threshold as usize {
				break
			}
		}

		let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
			DKGId,
			MaxSignatureLength,
			MaxReporters,
		> = AggregatedMisbehaviourReports {
			misbehaviour_type,
			session_id,
			offender: offender.clone(),
			reporters: reporters.clone().try_into().unwrap(),
			signatures: signatures.try_into().unwrap(),
		};

		// should not be accepted since we did not have t+1 signers
		assert_noop!(
			DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports.clone()
			),
			Error::<Test>::InvalidMisbehaviourReports
		);
	});
}

// Test that the misbehaviour reports does not drop the thresholds if we have more than
// threshold authorities available
#[test]
fn keygen_misbehaviour_reports_does_not_drop_threshold_if_authorities_available() {
	new_test_ext(vec![1, 2, 3, 4, 5]).execute_with(|| {
		let session_id = 1;
		// setup the new threshold so our signature is accepted
		// we have 5 authorities, but use lower thresholds for the test
		let keygen_threshold = 3;
		let signature_threshold = 2;
		NextKeygenThreshold::<Test>::put(keygen_threshold);
		NextSignatureThreshold::<Test>::put(signature_threshold);

		// prep the next authorities
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut next_authorities_raw: Vec<_> = Default::default();
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let dkg_id = DKGId::from(authority_id);
			next_authorities_raw.push(authority_id);
			next_authorities.try_push(dkg_id).unwrap();
		}
		NextAuthorities::<Test>::put(&next_authorities);
		let next_best_authorities =
			DKGMetadata::get_best_authorities(keygen_threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities: BoundedVec<_, _> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<Test>::put(&bounded_next_best_authorities);

		// 1 is the misbehaving party
		let offender: DKGId = next_authorities.first().unwrap().clone();
		// lets give the offender some reputation
		AuthorityReputations::<Test>::insert(offender.clone(), 100);

		let mut reporters: Vec<DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let misbehaviour_type = MisbehaviourType::Keygen;

		for authority_id in next_authorities_raw.iter() {
			let dkg_id = DKGId::from(authority_id.clone());

			if dkg_id == offender {
				continue
			}

			let sig = mock_misbehaviour_report::<Test>(
				authority_id.clone(),
				offender.clone(),
				misbehaviour_type,
			);
			signatures.try_push(sig).unwrap();
			reporters.push(dkg_id.clone());
		}

		let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
			DKGId,
			MaxSignatureLength,
			MaxReporters,
		> = AggregatedMisbehaviourReports {
			misbehaviour_type,
			session_id,
			offender: offender.clone(),
			reporters: reporters.clone().try_into().unwrap(),
			signatures: signatures.try_into().unwrap(),
		};

		assert_ok!(DKGMetadata::submit_misbehaviour_reports(
			RuntimeOrigin::none(),
			aggregated_misbehaviour_reports
		));

		// succcessful submission event should be present
		assert_last_event::<Test>(
			Event::MisbehaviourReportsSubmitted {
				misbehaviour_type,
				reporters: reporters.clone().try_into().unwrap(),
				offender: offender.clone(),
			}
			.into(),
		);

		// succcessful submission event should be present
		assert_has_event::<Test>(
			Event::AuthorityJailed { misbehaviour_type, authority: offender.clone() }.into(),
		);

		// lets check the storage is as expected

		// jailed authorities should contain the offender
		assert_eq!(JailedKeygenAuthorities::<Test>::get(offender), 1);

		// the threshold should remain unchanged
		assert_eq!(NextKeygenThreshold::<Test>::get(), keygen_threshold);
		assert_eq!(NextSignatureThreshold::<Test>::get(), signature_threshold);
	});
}

// Test that the misbehaviour reports does not drop the thresholds lower than 2
#[test]
fn keygen_misbehaviour_reports_does_not_drop_threshold_below_2() {
	new_test_ext(vec![1, 2, 3]).execute_with(|| {
		let session_id = 1;
		// setup the new threshold so our signature is accepted
		let keygen_threshold = 5;
		let signature_threshold = 3;
		NextKeygenThreshold::<Test>::put(keygen_threshold);
		NextSignatureThreshold::<Test>::put(signature_threshold);

		// prep the next authorities
		let mut next_authorities: BoundedVec<_, _> = Default::default();
		let mut next_authorities_raw: Vec<_> = Default::default();
		for _ in 1..=5 {
			let authority_id = mock_pub_key();
			let dkg_id = DKGId::from(authority_id);
			next_authorities_raw.push(authority_id);
			next_authorities.try_push(dkg_id).unwrap();
		}

		// load them onchain
		NextAuthorities::<Test>::put(&next_authorities);
		let next_best_authorities =
			DKGMetadata::get_best_authorities(keygen_threshold as usize, &next_authorities);
		let mut bounded_next_best_authorities: BoundedVec<_, _> = Default::default();
		for auth in next_best_authorities {
			bounded_next_best_authorities.try_push(auth).unwrap();
		}
		NextBestAuthorities::<Test>::put(&bounded_next_best_authorities);

		let mut offenders = vec![];

		// lets jail two authorities one after other
		for i in 0..3 {
			let offender: DKGId = next_authorities[i].clone();
			offenders.push(offender.clone());
			// lets give the offender some reputation
			AuthorityReputations::<Test>::insert(offender.clone(), 100);

			let mut reporters: Vec<DKGId> = Vec::new();
			let mut signatures: BoundedVec<_, _> = Default::default();
			let misbehaviour_type = MisbehaviourType::Keygen;

			// everyone except 1 reports the misbehaviour
			for authority_id in next_authorities_raw.iter() {
				let dkg_id = DKGId::from(authority_id.clone());

				if dkg_id == offender {
					continue
				}

				let sig = mock_misbehaviour_report::<Test>(
					authority_id.clone(),
					offender.clone(),
					misbehaviour_type,
				);
				signatures.try_push(sig).unwrap();
				reporters.push(dkg_id.clone());
			}

			let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
				DKGId,
				MaxSignatureLength,
				MaxReporters,
			> = AggregatedMisbehaviourReports {
				misbehaviour_type,
				session_id,
				offender: offender.clone(),
				reporters: reporters.clone().try_into().unwrap(),
				signatures: signatures.try_into().unwrap(),
			};

			assert_ok!(DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports
			));

			// succcessful submission event should be present
			assert_last_event::<Test>(
				Event::MisbehaviourReportsSubmitted {
					misbehaviour_type,
					reporters: reporters.clone().try_into().unwrap(),
					offender: offender.clone(),
				}
				.into(),
			);

			// succcessful submission event should be present
			assert_has_event::<Test>(
				Event::AuthorityJailed { misbehaviour_type, authority: offender.clone() }.into(),
			);
		}

		// lets check the storage is as expected

		// jailed authorities should contain both the offenders
		for offender in offenders.iter() {
			assert_eq!(JailedKeygenAuthorities::<Test>::get(offender), 1);
		}

		// next best authorities should contain everyone except the offenders
		let mut current_next_best_authorities = NextBestAuthorities::<Test>::get();
		assert_eq!(current_next_best_authorities.len() as u16, keygen_threshold - 3);
		current_next_best_authorities.retain(|x| offenders.contains(&x.1));
		assert_eq!(current_next_best_authorities.len(), 0);

		// the threshold should have reduced correctly, not gone below 2
		assert_eq!(NextKeygenThreshold::<Test>::get(), keygen_threshold - 3);

		// should correctly drop the signature threshold to ensure we do not end up in t = n
		assert_eq!(NextSignatureThreshold::<Test>::get(), signature_threshold - 2);

		// === we are not in effectively a 2-of-1 state, we should not be able to jail anyone else
		// ====

		let offender: DKGId = next_authorities[4].clone();
		let mut reporters: Vec<DKGId> = Vec::new();
		let mut signatures: BoundedVec<_, _> = Default::default();
		let misbehaviour_type = MisbehaviourType::Keygen;

		// everyone except offender reports the misbehaviour
		for authority_id in next_authorities_raw.iter() {
			let dkg_id = DKGId::from(authority_id.clone());

			if dkg_id == offender {
				continue
			}

			let sig = mock_misbehaviour_report::<Test>(
				authority_id.clone(),
				offender.clone(),
				misbehaviour_type,
			);
			signatures.try_push(sig).unwrap();
			reporters.push(dkg_id.clone());
		}

		let aggregated_misbehaviour_reports: AggregatedMisbehaviourReports<
			DKGId,
			MaxSignatureLength,
			MaxReporters,
		> = AggregatedMisbehaviourReports {
			misbehaviour_type,
			session_id,
			offender: offender.clone(),
			reporters: reporters.clone().try_into().unwrap(),
			signatures: signatures.try_into().unwrap(),
		};

		assert_noop!(
			DKGMetadata::submit_misbehaviour_reports(
				RuntimeOrigin::none(),
				aggregated_misbehaviour_reports
			),
			Error::<Test>::NotEnoughAuthoritiesToJail
		);
	});
}
