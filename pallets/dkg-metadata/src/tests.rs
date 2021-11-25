// Copyright (C) 2020 - 2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use codec::Decode;
use dkg_runtime_primitives::OFFCHAIN_PUBLIC_KEY;
use std::{sync::Arc, vec};

use codec::Encode;
use dkg_runtime_primitives::AuthoritySet;

use dkg_runtime_primitives::OFFCHAIN_PUBLIC_KEY_SIG;
use frame_support::{assert_err, assert_ok};
use sp_core::H256;
use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};
use sp_runtime::{
	offchain::{
		storage::StorageValueRef, testing, OffchainDbExt, OffchainStorage, OffchainWorkerExt,
		TransactionPoolExt, STORAGE_PREFIX,
	},
	DigestItem, RuntimeAppPublic,
};

use frame_support::traits::OnInitialize;

use crate::mock::*;

fn init_block(block: u64) {
	System::set_block_number(block);
	Session::on_initialize(block);
}

pub fn dkg_log(log: ConsensusLog<DKGId>) -> DigestItem<H256> {
	DigestItem::Consensus(DKG_ENGINE_ID, log.encode())
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
fn session_change_updates_authorities() {
	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(4);

		assert!(0 == DKGMetadata::authority_set_id());

		// no change - no log
		assert!(System::digest().logs.is_empty());

		init_block(8);

		assert!(1 == DKGMetadata::authority_set_id());

		let want = dkg_log(ConsensusLog::AuthoritiesChange {
			next_authorities: AuthoritySet {
				authorities: vec![mock_dkg_id(3), mock_dkg_id(4)],
				id: 1,
			},
			next_queued_authorities: AuthoritySet {
				authorities: vec![mock_dkg_id(3), mock_dkg_id(4)],
				id: 2,
			},
		});

		let log = System::digest().logs[0].clone();

		assert_eq!(want, log);
	});
}

#[test]
fn session_change_updates_next_authorities() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(4);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[0], next_authorities[0]);
		assert_eq!(want[1], next_authorities[1]);

		init_block(8);

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
fn authority_set_updates_work() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(4);

		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 0u64);
		assert_eq!(want[0], vs.authorities[0]);
		assert_eq!(want[1], vs.authorities[1]);

		init_block(8);

		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 1u64);
		assert_eq!(want[2], vs.authorities[0]);
		assert_eq!(want[3], vs.authorities[1]);
	});
}

#[test]
fn should_submit_public_key() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let pub_key = vec![0u8; 65];

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		let pub_key_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY);

		pub_key_ref.set(&pub_key);

		assert_ok!(DKGMetadata::submit_public_key_onchain(0));

		let tx = pool_state.write().transactions.pop().unwrap();
		assert!(pool_state.read().transactions.is_empty());
		let tx = Extrinsic::decode(&mut &*tx).unwrap();
		assert_eq!(tx.signature.unwrap().0, 0);
		assert_eq!(tx.call, Call::DKGMetadata(crate::Call::submit_public_key { pub_key }));

		assert_eq!(pub_key_ref.get::<Vec<u8>>(), Ok(None));
	});
}

#[test]
fn should_not_submit_next_public_key_if_it_has_been_set_onchain() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let pub_key = vec![0u8; 65];
	let onchain_key = vec![2u8; 65];

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		let pub_key_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY);

		crate::pallet::NextDKGPublicKey::<Test>::put((0, &onchain_key));

		pub_key_ref.set(&pub_key);

		assert_ok!(DKGMetadata::submit_public_key_onchain(0));

		assert!(pool_state.read().transactions.is_empty());

		assert_eq!(pub_key_ref.get::<Vec<u8>>(), Ok(None));
	});
}

#[test]
fn should_submit_non_existing_public_key_signature() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let signature = vec![0u8; 65];

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		let pub_sig_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY_SIG);

		pub_sig_ref.set(&signature);

		assert_ok!(DKGMetadata::submit_public_key_signature_onchain(0));

		let tx = pool_state.write().transactions.pop().unwrap();
		assert!(pool_state.read().transactions.is_empty());
		let tx = Extrinsic::decode(&mut &*tx).unwrap();
		assert_eq!(tx.signature.unwrap().0, 0);
		assert_eq!(
			tx.call,
			Call::DKGMetadata(crate::Call::submit_public_key_signature { signature })
		);

		assert_eq!(pub_sig_ref.get::<Vec<u8>>(), Ok(None));
	});
}

#[test]
fn should_not_submit_existing_public_key_signature() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let signature = vec![0u8; 65];

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		crate::pallet::NextPublicKeySignature::<Test>::put((0, &signature));

		let pub_sig_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY_SIG);

		pub_sig_ref.set(&signature);

		assert_ok!(DKGMetadata::submit_public_key_signature_onchain(0));

		assert!(pool_state.read().transactions.is_empty());

		assert_eq!(pub_sig_ref.get::<Vec<u8>>(), Ok(None));
	});
}

#[test]
fn should_not_be_able_to_submit_multiple_keys_within_the_same_session() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let signature = vec![0u8; 65];

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		crate::pallet::NextPublicKeySignature::<Test>::put((0, &signature));

		let pub_sig_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY_SIG);

		pub_sig_ref.set(&signature);

		// Submit extrinsic  at block 0
		assert_ok!(DKGMetadata::submit_public_key_signature_onchain(0));

		assert!(pool_state.read().transactions.is_empty());

		assert_eq!(pub_sig_ref.get::<Vec<u8>>(), Ok(None));

		let new_key_signature = [2u8; 65];

		pub_sig_ref.set(&new_key_signature);
		// Sessions are four blocks long
		// Extrinsic submission should fail, since we are submitting at block 2
		assert_err!(
			DKGMetadata::submit_public_key_signature_onchain(2),
			"Already submitted public key signature in this session"
		);

		// Extrinsic submission should be ok
		assert_ok!(DKGMetadata::submit_public_key_signature_onchain(5));
	});
}
