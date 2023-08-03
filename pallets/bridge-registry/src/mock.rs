#![allow(clippy::unwrap_used)]
use super::*;
use crate as pallet_bridge_registry;

use codec::{Decode, Encode};
use frame_support::{parameter_types, traits::GenesisBuild};
use frame_system as system;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentifyAccount, IdentityLookup, Verify},
	AccountId32, BuildStorage, MultiSignature,
};
use sp_std::convert::{TryFrom, TryInto};

pub type Signature = MultiSignature;
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		BridgeRegistry: pallet_bridge_registry,
		Balances: pallet_balances,
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

impl system::Config for Test {
	type AccountData = pallet_balances::AccountData<u128>;
	type AccountId = AccountId;
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockHashCount = BlockHashCount;
	type BlockLength = ();
	type Block = frame_system::mocking::MockBlock<Test>;
	type BlockWeights = ();
	type RuntimeCall = RuntimeCall;
	type DbWeight = ();
	type RuntimeEvent = RuntimeEvent;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type Nonce = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type OnKilledAccount = ();
	type OnNewAccount = ();
	type OnSetCode = ();
	type RuntimeOrigin = RuntimeOrigin;
	type PalletInfo = PalletInfo;
	type SS58Prefix = SS58Prefix;
	type SystemWeightInfo = ();
	type Version = ();
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 1;
}

impl pallet_balances::Config for Test {
	type AccountStore = System;
	type Balance = u128;
	type DustRemoval = ();
	type RuntimeEvent = RuntimeEvent;
	type ExistentialDeposit = ExistentialDeposit;
	type MaxLocks = ();
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type FreezeIdentifier = ();
	type RuntimeHoldReason = RuntimeHoldReason;
	type MaxHolds = ();
	type MaxFreezes = ();
	type WeightInfo = ();
}

parameter_types! {
	#[derive(serde::Serialize, serde::Deserialize)]
	pub const MaxAdditionalFields: u32 = 5;
	#[derive(serde::Serialize, serde::Deserialize)]
	pub const MaxResources: u32 = 32;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxProposalLength : u32 = 10_000;
}

impl pallet_bridge_registry::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BridgeIndex = u32;
	type MaxAdditionalFields = MaxAdditionalFields;
	type MaxResources = MaxResources;
	type ForceOrigin = frame_system::EnsureRoot<AccountId>;
	type MaxProposalLength = MaxProposalLength;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = RuntimeGenesisConfig::default().build_storage().unwrap();
	let _ = pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(AccountId32::new([1u8; 32]), 10u128.pow(18)),
			(AccountId32::new([2u8; 32]), 20u128.pow(18)),
			(AccountId32::new([3u8; 32]), 30u128.pow(18)),
		],
	}
	.assimilate_storage(&mut storage);
	let _ = pallet_bridge_registry::GenesisConfig::<Test> {
		phantom: Default::default(),
		bridges: vec![],
	}
	.assimilate_storage(&mut storage);

	storage.into()
}
