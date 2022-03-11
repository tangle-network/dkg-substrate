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
#![cfg(test)]

use super::*;

use crate::{self as pallet_dkg_proposals};
use frame_support::{
	assert_ok, ord_parameter_types, parameter_types,
	traits::{GenesisBuild, OnFinalize, OnInitialize},
	PalletId,
};
use frame_system::{self as system};
pub use pallet_balances;
use sp_core::{sr25519::Signature, H256};
use sp_runtime::{
	app_crypto::{ecdsa::Public, sr25519},
	testing::{Header, TestXt},
	traits::{
		AccountIdConversion, BlakeTwo256, ConvertInto, Extrinsic as ExtrinsicT, IdentifyAccount,
		IdentityLookup, OpaqueKeys, Verify,
	},
	Permill,
};

use dkg_runtime_primitives::crypto::AuthorityId as DKGId;

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

sp_runtime::impl_opaque_keys! {
	pub struct MockSessionKeys {
		pub dkg: DKGMetadata,
	}
}

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
		Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
		CollatorSelection: pallet_collator_selection::{Pallet, Call, Storage, Event<T>, Config<T>},
		Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
		DKGMetadata: pallet_dkg_metadata::{Pallet, Call, Config<T>, Event<T>, Storage},
		Aura: pallet_aura::{Pallet, Storage, Config<T>},
		DKGProposals: pallet_dkg_proposals::{Pallet, Call, Storage, Event<T>},
		DKGProposalHandler: pallet_dkg_proposal_handler::{Pallet, Call, Storage, Event<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl system::Config for Test {
	type AccountData = pallet_balances::AccountData<u64>;
	type AccountId = AccountId;
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockHashCount = BlockHashCount;
	type BlockLength = ();
	type BlockNumber = u64;
	type BlockWeights = ();
	type Call = Call;
	type DbWeight = ();
	type Event = Event;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type Header = Header;
	type Index = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type OnKilledAccount = ();
	type OnNewAccount = ();
	type OnSetCode = ();
	type Origin = Origin;
	type PalletInfo = PalletInfo;
	type SS58Prefix = SS58Prefix;
	type SystemWeightInfo = ();
	type Version = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 1;
}

ord_parameter_types! {
	pub const One: u64 = 1;
}

impl pallet_balances::Config for Test {
	type AccountStore = System;
	type Balance = u64;
	type DustRemoval = ();
	type Event = Event;
	type ExistentialDeposit = ExistentialDeposit;
	type MaxLocks = ();
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type WeightInfo = ();
}

parameter_types! {
	pub const ChainIdentifier: ChainIdType<u32> = ChainIdType::Substrate(5);
	pub const ProposalLifetime: u64 = 50;
	pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
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
	type OnAuthoritySetChangeHandler = DKGProposals;
	type OnDKGPublicKeyChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type RefreshDelay = RefreshDelay;
	type TimeToRestart = TimeToRestart;
	type ProposalHandler = ();
}

parameter_types! {
	pub const MinimumPeriod: u64 = 1;
	pub const RefreshDelay: Permill = Permill::from_percent(90);
	pub const TimeToRestart: u64 = 3;
}

impl pallet_timestamp::Config for Test {
	type MinimumPeriod = MinimumPeriod;
	type Moment = u64;
	type OnTimestampSet = Aura;
	type WeightInfo = ();
}

impl pallet_aura::Config for Test {
	type AuthorityId = sp_consensus_aura::sr25519::AuthorityId;
	type DisabledValidators = ();
	type MaxAuthorities = MaxAuthorities;
}

parameter_types! {
	pub const Period: u32 = 10;
	pub const Offset: u32 = 0;
	pub const MaxAuthorities: u32 = 100_000;
}

impl pallet_session::Config for Test {
	type Event = Event;
	type ValidatorId = AccountId;
	type ValidatorIdOf = ConvertInto;
	type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type SessionManager = CollatorSelection;
	type SessionHandler = <MockSessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = MockSessionKeys;
	type WeightInfo = ();
}

parameter_types! {
	pub const PotId: PalletId = PalletId(*b"PotStake");
	pub const MaxCandidates: u32 = 1000;
	pub const MinCandidates: u32 = 0;
	pub const MaxInvulnerables: u32 = 100;
}

impl pallet_collator_selection::Config for Test {
	type Event = Event;
	type Currency = Balances;
	type UpdateOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type PotId = PotId;
	type MaxCandidates = MaxCandidates;
	type MinCandidates = MinCandidates;
	type MaxInvulnerables = MaxInvulnerables;
	// should be a multiple of session or things will get inconsistent
	type KickThreshold = Period;
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_collator_selection::IdentityCollator;
	type ValidatorRegistration = Session;
	type WeightInfo = ();
}

impl pallet_dkg_proposal_handler::Config for Test {
	type Event = Event;
	type ChainId = u32;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type MaxSubmissionsPerBatch = frame_support::traits::ConstU16<100>;
	type WeightInfo = ();
}

impl pallet_dkg_proposals::Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAccountId = DKGAccountId;
	type ChainId = u32;
	type ChainIdentifier = ChainIdentifier;
	type Event = Event;
	type Proposal = Vec<u8>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
	type WeightInfo = ();
}

pub fn mock_dkg_id(id: u8) -> DKGId {
	DKGId::from(Public::from_raw([id; 33]))
}

pub fn mock_pub_key(id: u8) -> AccountId {
	sr25519::Public::from_raw([id; 32])
}

pub(crate) fn roll_to(n: u64) {
	while System::block_number() < n {
		Balances::on_finalize(System::block_number());
		CollatorSelection::on_finalize(System::block_number());
		Session::on_finalize(System::block_number());
		Aura::on_finalize(System::block_number());
		System::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
		Timestamp::on_initialize(System::block_number());
		Balances::on_initialize(System::block_number());
		CollatorSelection::on_initialize(System::block_number());
		Session::on_initialize(System::block_number());
		Aura::on_initialize(System::block_number());
	}

	Session::rotate_session();
}

pub fn dkg_session_keys(dkg_keys: DKGId) -> MockSessionKeys {
	MockSessionKeys { dkg: dkg_keys }
}

// pub const BRIDGE_ID: u64 =
pub const PROPOSER_A: u8 = 1;
pub const PROPOSER_B: u8 = 2;
pub const PROPOSER_C: u8 = 3;
pub const PROPOSER_D: u8 = 4;
pub const SMALL_BALANCE: u64 = 100_000;
pub const ENDOWED_BALANCE: u64 = 100_000_000;
pub const TEST_THRESHOLD: u32 = 2;

pub struct ExtBuilder;

impl ExtBuilder {
	pub fn build() -> sp_io::TestExternalities {
		let dkg_id = PalletId(*b"dw/dkgac").into_account();
		let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
		pallet_balances::GenesisConfig::<Test> { balances: vec![(dkg_id, ENDOWED_BALANCE)] }
			.assimilate_storage(&mut t)
			.unwrap();
		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	pub fn with_genesis_collators() -> sp_io::TestExternalities {
		let dkg_id = PalletId(*b"dw/dkgac").into_account();
		let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
		let candidates = vec![
			(mock_pub_key(PROPOSER_A), mock_dkg_id(PROPOSER_A), 1000),
			(mock_pub_key(PROPOSER_B), mock_dkg_id(PROPOSER_B), 1000),
			(mock_pub_key(PROPOSER_C), mock_dkg_id(PROPOSER_C), 1000),
			(mock_pub_key(PROPOSER_D), mock_dkg_id(PROPOSER_D), 1000),
		];
		pallet_balances::GenesisConfig::<Test> {
			balances: vec![
				(dkg_id, ENDOWED_BALANCE),
				(mock_pub_key(PROPOSER_A), ENDOWED_BALANCE),
				(mock_pub_key(PROPOSER_B), ENDOWED_BALANCE),
				(mock_pub_key(PROPOSER_C), ENDOWED_BALANCE),
				(mock_pub_key(PROPOSER_D), ENDOWED_BALANCE),
			],
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_collator_selection::GenesisConfig::<Test> {
			invulnerables: vec![
				mock_pub_key(PROPOSER_A),
				mock_pub_key(PROPOSER_B),
				mock_pub_key(PROPOSER_C),
			],
			candidacy_bond: SMALL_BALANCE,
			desired_candidates: 4,
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_session::GenesisConfig::<Test> {
			keys: candidates
				.iter()
				.cloned()
				.map(|(acc, dkg, _)| {
					(
						acc.clone(),           // account id
						acc.clone(),           // validator id
						dkg_session_keys(dkg), // session keys
					)
				})
				.collect(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| {
			let _ =
				CollatorSelection::register_as_candidate(Origin::signed(mock_pub_key(PROPOSER_D)));
			System::set_block_number(1);
		});
		ext
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	ExtBuilder::build()
}

pub fn new_test_ext_initialized(
	src_id: ChainIdType<<Test as pallet::Config>::ChainId>,
	r_id: ResourceId,
	resource: Vec<u8>,
) -> sp_io::TestExternalities {
	let mut t = ExtBuilder::build();
	t.execute_with(|| {
		// Set and check threshold
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);
		// Add proposers
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_A)));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_B)));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_C)));
		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), src_id));
		// Set and check resource ID mapped to some junk data
		assert_ok!(DKGProposals::set_resource(Origin::root(), r_id, resource));
		assert_eq!(DKGProposals::resource_exists(r_id), true);
	});
	t
}

// Checks events against the latest. A contiguous set of events must be
// provided. They must include the most recent event, but do not have to include
// every past event.
pub fn assert_events(mut expected: Vec<Event>) {
	let mut actual: Vec<Event> =
		system::Pallet::<Test>::events().iter().map(|e| e.event.clone()).collect();

	expected.reverse();
	for evt in expected {
		let next = actual.pop().expect("event expected");
		assert_eq!(next, evt.into(), "Events don't match (actual,expected)");
	}
}

pub fn assert_has_event(ev: Event) -> () {
	let actual: Vec<Event> =
		system::Pallet::<Test>::events().iter().map(|e| e.event.clone()).collect();
	assert!(actual.contains(&ev))
}

pub fn assert_does_not_have_event(ev: Event) -> () {
	let actual: Vec<Event> =
		system::Pallet::<Test>::events().iter().map(|e| e.event.clone()).collect();

	assert!(!actual.contains(&ev))
}
