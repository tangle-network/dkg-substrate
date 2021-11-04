#![cfg(test)]

use super::*;

use frame_support::{assert_ok, ord_parameter_types, parameter_types, PalletId};
use frame_system::{self as system};
use sp_core::H256;
use sp_runtime::{
	testing::Header,
	traits::{AccountIdConversion, BlakeTwo256, IdentityLookup},
};

use crate::{self as pallet_dkg_proposals, Config};
pub use pallet_balances;

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
		DKGProposals: pallet_dkg_proposals::{Pallet, Call, Storage, Event<T>},
		DKGProposalHandler: pallet_dkg_proposal_handler::{Pallet, Call, Storage, Event<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

impl system::Config for Test {
	type AccountData = pallet_balances::AccountData<u64>;
	type AccountId = u64;
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
	pub const ChainIdentifier: u32 = 5;
	pub const ProposalLifetime: u64 = 50;
	pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
}

impl pallet_dkg_proposal_handler::Config for Test {
	type Event = Event;
	type Proposal = Vec<u8>;
}

impl Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAccountId = DKGAccountId;
	type ChainId = u32;
	type ChainIdentifier = ChainIdentifier;
	type Event = Event;
	type Proposal = Vec<u8>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
}

// pub const BRIDGE_ID: u64 =
pub const RELAYER_A: u64 = 0x2;
pub const RELAYER_B: u64 = 0x3;
pub const RELAYER_C: u64 = 0x4;
pub const ENDOWED_BALANCE: u64 = 100_000_000;
pub const TEST_THRESHOLD: u32 = 2;

pub fn new_test_ext() -> sp_io::TestExternalities {
	let dkg_id = PalletId(*b"dw/dkgac").into_account();
	let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
	pallet_balances::GenesisConfig::<Test> { balances: vec![(dkg_id, ENDOWED_BALANCE)] }
		.assimilate_storage(&mut t)
		.unwrap();
	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1));
	ext
}

pub fn new_test_ext_initialized(
	src_id: <Test as pallet::Config>::ChainId,
	r_id: ResourceId,
	resource: Vec<u8>,
) -> sp_io::TestExternalities {
	let mut t = new_test_ext();
	t.execute_with(|| {
		// Set and check threshold
		assert_ok!(DKGProposals::set_threshold(Origin::root(), TEST_THRESHOLD));
		assert_eq!(DKGProposals::relayer_threshold(), TEST_THRESHOLD);
		// Add relayers
		assert_ok!(DKGProposals::add_relayer(Origin::root(), RELAYER_A));
		assert_ok!(DKGProposals::add_relayer(Origin::root(), RELAYER_B));
		assert_ok!(DKGProposals::add_relayer(Origin::root(), RELAYER_C));
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
