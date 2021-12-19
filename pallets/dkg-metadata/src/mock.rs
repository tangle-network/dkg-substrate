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

// construct_runtime requires this
#![allow(clippy::from_over_into)]

use std::vec;

use frame_support::{
	construct_runtime, parameter_types, sp_io::TestExternalities, traits::GenesisBuild,
	BasicExternalities,
};
use sp_core::{
	sr25519::{self, Signature},
	H256,
};
use sp_runtime::{
	app_crypto::ecdsa::Public,
	impl_opaque_keys,
	testing::{Header, TestXt},
	traits::{
		BlakeTwo256, ConvertInto, Extrinsic as ExtrinsicT, IdentifyAccount, IdentityLookup,
		OpaqueKeys, Verify,
	},
	Perbill, Permill,
};

use crate as pallet_dkg_metadata;
pub use dkg_runtime_primitives::{crypto::AuthorityId as DKGId, ConsensusLog, DKG_ENGINE_ID};

impl_opaque_keys! {
	pub struct MockSessionKeys {
		pub dummy: pallet_dkg_metadata::Pallet<Test>,
	}
}

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		DKGMetadata: pallet_dkg_metadata::{Pallet, Call, Config<T>, Event<T>, Storage},
		Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type Origin = Origin;
	type Index = u64;
	type BlockNumber = u64;
	type Hash = H256;
	type Call = Call;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Header = Header;
	type Event = Event;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

type Extrinsic = TestXt<Call, ()>;

impl frame_system::offchain::SigningTypes for Test {
	type Public = <Signature as Verify>::Signer;
	type Signature = Signature;
}

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for Test
where
	Call: From<LocalCall>,
{
	type OverarchingCall = Call;
	type Extrinsic = Extrinsic;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Test
where
	Call: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: Call,
		_public: <Signature as Verify>::Signer,
		_account: AccountId,
		nonce: u64,
	) -> Option<(Call, <Extrinsic as ExtrinsicT>::SignaturePayload)> {
		Some((call, (nonce, ())))
	}
}

impl pallet_dkg_metadata::Config for Test {
	type DKGId = DKGId;
	type Event = Event;
	type OnAuthoritySetChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain_crypto::OffchainAuthId;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type RefreshDelay = RefreshDelay;
}

parameter_types! {
	pub const Period: u64 = 1;
	pub const Offset: u64 = 0;
	pub const RefreshDelay: Permill = Permill::from_percent(90);
}

impl pallet_session::Config for Test {
	type Event = Event;
	type ValidatorId = AccountId;
	type ValidatorIdOf = ConvertInto;
	type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type SessionManager = MockSessionManager;
	type SessionHandler = <MockSessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = MockSessionKeys;
	type WeightInfo = ();
}

pub struct MockSessionManager;

impl pallet_session::SessionManager<AccountId> for MockSessionManager {
	fn end_session(_: sp_staking::SessionIndex) {}
	fn start_session(_: sp_staking::SessionIndex) {}
	fn new_session(idx: sp_staking::SessionIndex) -> Option<Vec<AccountId>> {
		if idx == 0 || idx == 1 {
			Some(vec![mock_pub_key(1), mock_pub_key(2)])
		} else if idx == 2 {
			Some(vec![mock_pub_key(3), mock_pub_key(4)])
		} else {
			None
		}
	}
}

// Note, that we can't use `UintAuthorityId` here. Reason is that the implementation
// of `to_public_key()` assumes, that a public key is 32 bytes long. This is true for
// ed25519 and sr25519 but *not* for ecdsa. An ecdsa public key is 33 bytes.
pub fn mock_dkg_id(id: u8) -> DKGId {
	DKGId::from(Public::from_raw([id; 33]))
}

pub fn mock_pub_key(id: u8) -> AccountId {
	sr25519::Public::from_raw([id; 32])
}

pub fn mock_authorities(vec: Vec<u8>) -> Vec<(AccountId, DKGId)> {
	vec.into_iter().map(|id| (mock_pub_key(id), mock_dkg_id(id))).collect()
}

pub fn new_test_ext(ids: Vec<u8>) -> TestExternalities {
	new_test_ext_raw_authorities(mock_authorities(ids))
}

pub fn new_test_ext_raw_authorities(authorities: Vec<(AccountId, DKGId)>) -> TestExternalities {
	let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();

	let session_keys: Vec<_> = authorities
		.iter()
		.enumerate()
		.map(|(_, id)| (id.0.clone(), id.0.clone(), MockSessionKeys { dummy: id.1.clone() }))
		.collect();

	BasicExternalities::execute_with_storage(&mut t, || {
		for (ref id, ..) in &session_keys {
			frame_system::Pallet::<Test>::inc_providers(id);
		}
	});

	pallet_session::GenesisConfig::<Test> { keys: session_keys }
		.assimilate_storage(&mut t)
		.unwrap();

	t.into()
}
