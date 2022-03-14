#![cfg(test)]

use super::*;

use crate::{self as pallet_dkg_proposals, Config};
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
	Perbill, Percent, Permill,
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
		ParachainStaking: pallet_parachain_staking::{Pallet, Call, Storage, Config<T>, Event<T>},
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
	pub const ChainIdentifier: TypedChainId = TypedChainId::Substrate(5);
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
	type NextSessionRotation = ParachainStaking;
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

parameter_types! {
	pub const MaxAuthorities: u32 = 100_000;
}

impl pallet_aura::Config for Test {
	type AuthorityId = sp_consensus_aura::sr25519::AuthorityId;
	type DisabledValidators = ();
	type MaxAuthorities = MaxAuthorities;
}

impl pallet_session::Config for Test {
	type Event = Event;
	type ValidatorId = AccountId;
	type ValidatorIdOf = ConvertInto;
	type ShouldEndSession = ParachainStaking;
	type NextSessionRotation = ParachainStaking;
	type SessionManager = ParachainStaking;
	type SessionHandler = <MockSessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = MockSessionKeys;
	type WeightInfo = ();
}

parameter_types! {
	pub const MinBlocksPerRound: u32 = 3;
	pub const BlocksPerRound: u32 = 5;
	pub const LeaveCandidatesDelay: u32 = 2;
	pub const LeaveNominatorsDelay: u32 = 2;
	pub const RevokeNominationDelay: u32 = 2;
	pub const RewardPaymentDelay: u32 = 2;
	pub const MinSelectedCandidates: u32 = 5;
	pub const MaxNominatorsPerCollator: u32 = 4;
	pub const MaxCollatorsPerNominator: u32 = 4;
	pub const DefaultCollatorCommission: Perbill = Perbill::from_percent(20);
	pub const DefaultParachainBondReservePercent: Percent = Percent::from_percent(30);
	pub const MinCollatorStk: u64 = 10;
	pub const MinNominatorStk: u64 = 5;
	pub const MinNomination: u64 = 3;
	pub const ParachainStakingPalletId: PalletId = PalletId(*b"dw/pcstk");
}

impl pallet_parachain_staking::Config for Test {
	type BlocksPerRound = BlocksPerRound;
	type PalletId = ParachainStakingPalletId;
	type Currency = Balances;
	type DefaultCollatorCommission = DefaultCollatorCommission;
	type DefaultParachainBondReservePercent = DefaultParachainBondReservePercent;
	type Event = Event;
	type LeaveCandidatesDelay = LeaveCandidatesDelay;
	type LeaveNominatorsDelay = LeaveNominatorsDelay;
	type MaxCollatorsPerNominator = MaxCollatorsPerNominator;
	type MaxNominatorsPerCollator = MaxNominatorsPerCollator;
	type MinBlocksPerRound = MinBlocksPerRound;
	type MinCollatorCandidateStk = MinCollatorStk;
	type MinCollatorStk = MinCollatorStk;
	type MinNomination = MinNomination;
	type MinNominatorStk = MinNominatorStk;
	type MinSelectedCandidates = MinSelectedCandidates;
	type MonetaryGovernanceOrigin = frame_system::EnsureRoot<AccountId>;
	type RevokeNominationDelay = RevokeNominationDelay;
	type RewardPaymentDelay = RewardPaymentDelay;
	type WeightInfo = ();
}

impl pallet_dkg_proposal_handler::Config for Test {
	type Event = Event;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type MaxSubmissionsPerBatch = frame_support::traits::ConstU16<100>;
	type WeightInfo = ();
}

impl pallet_dkg_proposals::Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAccountId = DKGAccountId;
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
		ParachainStaking::on_finalize(System::block_number());
		Session::on_finalize(System::block_number());
		Aura::on_finalize(System::block_number());
		System::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
		Timestamp::on_initialize(System::block_number());
		Balances::on_initialize(System::block_number());
		ParachainStaking::on_initialize(System::block_number());
		Session::on_initialize(System::block_number());
		Aura::on_initialize(System::block_number());
	}
}

pub fn dkg_session_keys(dkg_keys: DKGId) -> MockSessionKeys {
	MockSessionKeys { dkg: dkg_keys }
}

// pub const BRIDGE_ID: u64 =
pub const PROPOSER_A: u8 = 2;
pub const PROPOSER_B: u8 = 3;
pub const PROPOSER_C: u8 = 4;
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
			(mock_pub_key(0), mock_dkg_id(0), 1000),
			(mock_pub_key(1), mock_dkg_id(1), 1000),
			(mock_pub_key(2), mock_dkg_id(2), 1000),
			(mock_pub_key(3), mock_dkg_id(3), 1000),
		];
		pallet_balances::GenesisConfig::<Test> {
			balances: vec![
				(dkg_id, ENDOWED_BALANCE),
				(mock_pub_key(0), ENDOWED_BALANCE),
				(mock_pub_key(1), ENDOWED_BALANCE),
				(mock_pub_key(2), ENDOWED_BALANCE),
				(mock_pub_key(3), ENDOWED_BALANCE),
				(mock_pub_key(4), ENDOWED_BALANCE),
			],
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_parachain_staking::GenesisConfig::<Test> {
			nominations: vec![],
			candidates: candidates
				.iter()
				.cloned()
				.map(|(account, _, bond)| (account, bond))
				.collect(),
			inflation_config: Default::default(),
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

		let ext = sp_io::TestExternalities::new(t);
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
	let mut t = new_test_ext();
	t.execute_with(|| {
		// Set and check threshold
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::proposer_threshold(), TEST_THRESHOLD);
		// Add proposers
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_A)));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_B)));
		assert_ok!(DKGProposals::add_proposer(Origin::root(), mock_pub_key(PROPOSER_C)));
		// Whitelist chain
		assert_ok!(DKGProposals::whitelist_chain(Origin::root(), src_chain_id));
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
