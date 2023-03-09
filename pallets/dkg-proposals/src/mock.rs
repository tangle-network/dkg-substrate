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
use crate as pallet_dkg_proposals;
use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{
	assert_ok, ord_parameter_types,
	pallet_prelude::ConstU32,
	parameter_types,
	traits::{GenesisBuild, OnFinalize, OnInitialize},
	PalletId,
};
use frame_system as system;
pub use pallet_balances;
use sp_core::{sr25519::Signature, H256};
use sp_runtime::{
	app_crypto::{ecdsa::Public, sr25519},
	testing::{Header, TestXt},
	traits::{
		AccountIdConversion, BlakeTwo256, ConvertInto, Extrinsic as ExtrinsicT, IdentifyAccount,
		IdentityLookup, OpaqueKeys, Verify,
	},
	Percent, Permill,
};

use dkg_runtime_primitives::{crypto::AuthorityId as DKGId, TypedChainId};

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

pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl system::Config for Test {
	type AccountData = pallet_balances::AccountData<u64>;
	type AccountId = AccountId;
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockHashCount = BlockHashCount;
	type BlockLength = ();
	type BlockNumber = u64;
	type BlockWeights = ();
	type RuntimeCall = RuntimeCall;
	type DbWeight = ();
	type RuntimeEvent = RuntimeEvent;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type Header = Header;
	type Index = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type OnKilledAccount = ();
	type OnNewAccount = ();
	type OnSetCode = ();
	type RuntimeOrigin = RuntimeOrigin;
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
	type RuntimeEvent = RuntimeEvent;
	type ExistentialDeposit = ExistentialDeposit;
	type MaxLocks = ();
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type WeightInfo = ();
}

parameter_types! {
	pub const DecayPercentage: Percent = Percent::from_percent(50);
	pub const ChainIdentifier: TypedChainId = TypedChainId::Substrate(5);
	pub const ProposalLifetime: u64 = 50;
	pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
}

type Extrinsic = TestXt<RuntimeCall, ()>;

impl frame_system::offchain::SigningTypes for Test {
	type Public = <Signature as Verify>::Signer;
	type Signature = Signature;
}

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	type OverarchingCall = RuntimeCall;
	type Extrinsic = Extrinsic;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: RuntimeCall,
		_public: <Signature as Verify>::Signer,
		_account: AccountId,
		nonce: u64,
	) -> Option<(RuntimeCall, <Extrinsic as ExtrinsicT>::SignaturePayload)> {
		Some((call, (nonce, ())))
	}
}

parameter_types! {
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd, MaxEncodedLen, Default)]
	pub const MaxKeyLength : u32 = 10_000;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd, MaxEncodedLen, Default)]
	pub const MaxSignatureLength : u32 = 100;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd, MaxEncodedLen, Default)]
	pub const MaxAuthorities : u32 = 1000;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd, MaxEncodedLen, Default)]
	pub const MaxReporters : u32 = 1000;
}

impl pallet_dkg_metadata::Config for Test {
	type DKGId = DKGId;
	type RuntimeEvent = RuntimeEvent;
	type OnAuthoritySetChangeHandler = DKGProposals;
	type OnDKGPublicKeyChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type RefreshDelay = RefreshDelay;
	type KeygenJailSentence = Period;
	type SigningJailSentence = Period;
	type DecayPercentage = DecayPercentage;
	type SessionPeriod = Period;
	type Reputation = u128;
	type UnsignedInterval = frame_support::traits::ConstU64<0>;
	type UnsignedPriority = frame_support::traits::ConstU64<{ 1 << 20 }>;
	type AuthorityIdOf = pallet_dkg_metadata::AuthorityIdOf<Self>;
	type ProposalHandler = ();
	type MaxKeyLength = MaxKeyLength;
	type MaxSignatureLength = MaxSignatureLength;
	type MaxReporters = MaxReporters;
	type MaxAuthorities = MaxAuthorities;
	type WeightInfo = ();
}

pub const MILLISECS_PER_BLOCK: u64 = 10000;
pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

parameter_types! {
	pub const MinimumPeriod: u64 = SLOT_DURATION / 2;
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
}

impl pallet_session::Config for Test {
	type RuntimeEvent = RuntimeEvent;
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
	pub const KickThreshold: u32 = Period::get() * 10;
}

impl pallet_collator_selection::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type UpdateOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type PotId = PotId;
	type MaxCandidates = MaxCandidates;
	type MinCandidates = MinCandidates;
	type MaxInvulnerables = MaxInvulnerables;
	// should be a multiple of session or things will get inconsistent
	type KickThreshold = KickThreshold;
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_collator_selection::IdentityCollator;
	type ValidatorRegistration = Session;
	type WeightInfo = ();
}

parameter_types! {
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxProposalLength : u32 = 10_000;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxVotes : u32 = 100;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxResources : u32 = 1000;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxAuthorityProposers : u32 = 1000;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxExternalProposerAccounts : u32 = 1000;
}

impl pallet_dkg_proposal_handler::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type MaxSubmissionsPerBatch = frame_support::traits::ConstU16<100>;
	type UnsignedProposalExpiry = frame_support::traits::ConstU64<10>;
	type SignedProposalHandler = ();
	type MaxProposalLength = MaxProposalLength;
	type WeightInfo = ();
}

impl pallet_dkg_proposals::Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAuthorityToMerkleLeaf = DKGEcdsaToEthereum;
	type DKGId = DKGId;
	type ChainIdentifier = ChainIdentifier;
	type RuntimeEvent = RuntimeEvent;
	type Proposal = BoundedVec<u8, MaxProposalLength>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
	type Period = Period;
	type MaxVotes = MaxVotes;
	type MaxResources = MaxResources;
	type MaxAuthorityProposers = MaxAuthorityProposers;
	type MaxExternalProposerAccounts = MaxExternalProposerAccounts;
	type WeightInfo = ();
}

pub fn mock_dkg_id(id: u8) -> DKGId {
	let mut input = "0";
	if id == 1 {
		input = "039bb8e80670371f45508b5f8f59946a7c4dea4b3a23a036cf24c1f40993f4a1da";
	} else if id == 2 {
		input = "0350863ad64a87ae8a2fe83c1af1a8403cb53f53e486d8511dad8a04887e5b2352"
	} else if id == 3 {
		input = "03085ab8fbc19db189ced3410650737671cb8cad159bfa9f2c1d96ce1330f0978d"
	} else if id == 4 {
		input = "03d395ee0b30fa7d34f49ef9cb2868c8364e9d618a7f437101a979fb3c47f21de2"
	}
	let mut decoded_input = [0u8; 33];
	hex::decode_to_slice(input, &mut decoded_input).expect("decoding failed");
	DKGId::from(Public::from_raw(decoded_input))
}

pub fn mock_pub_key(id: u8) -> AccountId {
	sr25519::Public::from_raw([id; 32])
}

pub fn mock_ecdsa_key(id: u8) -> Vec<u8> {
	DKGEcdsaToEthereum::convert(mock_dkg_id(id))
}

pub(crate) fn roll_to(n: u64) {
	while System::block_number() < n {
		Balances::on_finalize(System::block_number());
		CollatorSelection::on_finalize(System::block_number());
		Session::on_finalize(System::block_number());
		Aura::on_finalize(System::block_number());
		System::on_finalize(System::block_number());
		DKGProposals::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
		Timestamp::on_initialize(System::block_number());
		Balances::on_initialize(System::block_number());
		Session::on_initialize(System::block_number());
		CollatorSelection::on_initialize(System::block_number());
		Aura::on_initialize(System::block_number());
		DKGProposals::on_initialize(System::block_number());
	}
}

pub fn dkg_session_keys(dkg_keys: DKGId) -> MockSessionKeys {
	MockSessionKeys { dkg: dkg_keys }
}

// pub const BRIDGE_ID: u64 =
pub const PROPOSER_A: u8 = 1;
pub const PROPOSER_B: u8 = 2;
pub const PROPOSER_C: u8 = 3;
pub const PROPOSER_D: u8 = 4;
pub const PROPOSER_E: u8 = 4;
pub const SMALL_BALANCE: u64 = 100_000;
pub const ENDOWED_BALANCE: u64 = 100_000_000;
pub const TEST_THRESHOLD: u32 = 2;

pub struct ExtBuilder;

impl ExtBuilder {
	pub fn build() -> sp_io::TestExternalities {
		let dkg_id = PalletId(*b"dw/dkgac").into_account_truncating();
		let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
		pallet_balances::GenesisConfig::<Test> { balances: vec![(dkg_id, ENDOWED_BALANCE)] }
			.assimilate_storage(&mut t)
			.unwrap();
		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	pub fn with_genesis_collators() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
		let candidates = vec![
			(mock_pub_key(PROPOSER_A), mock_dkg_id(PROPOSER_A), 1000),
			(mock_pub_key(PROPOSER_B), mock_dkg_id(PROPOSER_B), 1000),
			(mock_pub_key(PROPOSER_C), mock_dkg_id(PROPOSER_C), 1000),
			(mock_pub_key(PROPOSER_D), mock_dkg_id(PROPOSER_D), 1000),
		];
		pallet_balances::GenesisConfig::<Test> {
			balances: vec![
				(mock_pub_key(0), ENDOWED_BALANCE),
				(mock_pub_key(1), ENDOWED_BALANCE),
				(mock_pub_key(2), ENDOWED_BALANCE),
				(mock_pub_key(3), ENDOWED_BALANCE),
				(mock_pub_key(4), ENDOWED_BALANCE),
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
						acc,                   // account id
						acc,                   // validator id
						dkg_session_keys(dkg), // session keys
					)
				})
				.collect(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| {
			let _ = CollatorSelection::register_as_candidate(RuntimeOrigin::signed(mock_pub_key(
				PROPOSER_D,
			)));
			System::set_block_number(1);
		});
		ext
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	ExtBuilder::build()
}

pub fn new_test_ext_initialized(
	src_chain_id: TypedChainId,
	r_id: ResourceId,
	resource: Vec<u8>,
) -> sp_io::TestExternalities {
	let mut t = ExtBuilder::build();
	t.execute_with(|| {
		// Set and check threshold
		assert_ok!(DKGProposals::set_threshold(RuntimeOrigin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);
		// Add proposers
		assert_ok!(DKGProposals::add_proposer(
			RuntimeOrigin::root(),
			mock_pub_key(PROPOSER_A),
			mock_ecdsa_key(PROPOSER_A)
		));
		assert_ok!(DKGProposals::add_proposer(
			RuntimeOrigin::root(),
			mock_pub_key(PROPOSER_B),
			mock_ecdsa_key(PROPOSER_B)
		));
		assert_ok!(DKGProposals::add_proposer(
			RuntimeOrigin::root(),
			mock_pub_key(PROPOSER_C),
			mock_ecdsa_key(PROPOSER_C)
		));
		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(RuntimeOrigin::root(), src_chain_id));
		// Set and check resource ID mapped to some junk data
		assert_ok!(DKGProposals::set_resource(RuntimeOrigin::root(), r_id, resource));
		assert!(DKGProposals::resource_exists(r_id), "{}", true);
	});
	t
}

pub fn manually_set_proposer_count(count: u32) -> sp_io::TestExternalities {
	let mut t = new_test_ext();
	t.execute_with(|| {
		ProposerCount::<Test>::put(count);
	});
	t
}

// Checks events against the latest. A contiguous set of events must be
// provided. They must include the most recent RuntimeEvent, but do not have to include
// every past RuntimeEvent.
pub fn assert_events(mut expected: Vec<RuntimeEvent>) {
	let mut actual: Vec<RuntimeEvent> =
		system::Pallet::<Test>::events().iter().map(|e| e.event.clone()).collect();

	expected.reverse();
	for evt in expected {
		let next = actual.pop().expect("RuntimeEvent expected");
		assert_eq!(next, evt, "Events don't match (actual,expected)");
	}
}

pub fn assert_has_event(ev: RuntimeEvent) {
	println!("{ev:?}");
	println!("{:?}", system::Pallet::<Test>::events());
	assert!(system::Pallet::<Test>::events()
		.iter()
		.map(|e| e.event.clone())
		.any(|x| x == ev))
}

pub fn assert_does_not_have_event(ev: RuntimeEvent) {
	assert!(!system::Pallet::<Test>::events()
		.iter()
		.map(|e| e.event.clone())
		.any(|x| x == ev))
}
