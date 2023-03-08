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

use std::vec;

use crate::mock::*;
use frame_support::{assert_ok, traits::Hooks, weights::Weight, BoundedVec};
use sp_core::ByteArray;
use sp_runtime::traits::Bounded;

fn init_block(block: u64) {
	System::set_block_number(block);
	Session::on_initialize(block);
	DKGMetadata::on_initialize(block);
	DKGMetadata::on_idle(block, Weight::max_value());
	DKGMetadata::on_finalize(block);
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
		assert!(!DKGMetadata::should_execute_emergency_keygen());
		assert_ok!(DKGMetadata::trigger_emergency_keygen(RuntimeOrigin::root()));
		assert!(DKGMetadata::should_execute_emergency_keygen());
		System::set_block_number(2);
		DKGMetadata::on_initialize(2);
		// it should be reset to false after the block is initialized
		assert!(!DKGMetadata::should_execute_emergency_keygen());
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
