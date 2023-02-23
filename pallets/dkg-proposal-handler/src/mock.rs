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
use crate as pallet_dkg_proposal_handler;
use codec::Encode;
use frame_support::{parameter_types, traits::Everything, PalletId};
use frame_system as system;
use pallet_dkg_proposals::DKGEcdsaToEthereum;
use sp_core::{sr25519, sr25519::Signature, H256};
use sp_runtime::{
	impl_opaque_keys,
	testing::{Header, TestXt},
	traits::{
		BlakeTwo256, ConvertInto, Extrinsic as ExtrinsicT, IdentifyAccount, IdentityLookup,
		OpaqueKeys, Verify,
	},
	Percent, Permill,
};

use sp_core::offchain::{testing, OffchainDbExt, OffchainWorkerExt, TransactionPoolExt};

use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};

use sp_runtime::RuntimeAppPublic;

use dkg_runtime_primitives::{keccak_256, TransactionV2, TypedChainId};

use dkg_runtime_primitives::{
	crypto::AuthorityId as DKGId, EIP2930Transaction, TransactionAction, U256,
};
use std::sync::Arc;
use webb_proposals::{Proposal, ProposalKind};

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

impl_opaque_keys! {
	pub struct MockSessionKeys {
		pub dummy: pallet_dkg_metadata::Pallet<Test>,
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
		Session: pallet_session,
		DKG: pallet_dkg_metadata,
		Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
		DKGProposals: pallet_dkg_proposals::{Pallet, Call, Storage, Event<T>},
		DKGProposalHandler: pallet_dkg_proposal_handler::{Pallet, Call, Storage, Event<T>},
		Aura: pallet_aura::{Pallet, Storage, Config<T>},
	}
);

parameter_types! {
	pub const MaxAuthorities: u32 = 100_000;
}

impl pallet_aura::Config for Test {
	type AuthorityId = sp_consensus_aura::sr25519::AuthorityId;
	type DisabledValidators = ();
	type MaxAuthorities = MaxAuthorities;
}

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

parameter_types! {
	pub const ChainIdentifier: TypedChainId = TypedChainId::Substrate(5);
	pub const ProposalLifetime: u64 = 50;
	pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
}

type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl system::Config for Test {
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Index = u64;
	type BlockNumber = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Header = Header;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}
parameter_types! {
	pub const ExistentialDeposit: u64 = 1;
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

impl pallet_dkg_proposal_handler::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type MaxSubmissionsPerBatch = frame_support::traits::ConstU16<100>;
	type UnsignedProposalExpiry = frame_support::traits::ConstU64<10>;
	type SignedProposalHandler = ();
	type WeightInfo = ();
}

impl pallet_dkg_proposals::Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAuthorityToMerkleLeaf = DKGEcdsaToEthereum;
	type DKGId = DKGId;
	type ChainIdentifier = ChainIdentifier;
	type RuntimeEvent = RuntimeEvent;
	type Proposal = Vec<u8>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
	type Period = Period;
	type WeightInfo = ();
}

pub const MILLISECS_PER_BLOCK: u64 = 10000;
pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

parameter_types! {
	pub const MinimumPeriod: u64 = SLOT_DURATION / 2;
}

impl pallet_timestamp::Config for Test {
	type MinimumPeriod = MinimumPeriod;
	type Moment = u64;
	type OnTimestampSet = Aura;
	type WeightInfo = ();
}
pub struct MockSessionManager;

impl pallet_session::SessionManager<AccountId> for MockSessionManager {
	fn end_session(_: sp_staking::SessionIndex) {}
	fn start_session(_: sp_staking::SessionIndex) {}
	fn new_session(_idx: sp_staking::SessionIndex) -> Option<Vec<AccountId>> {
		None
	}
}

parameter_types! {
	pub const DecayPercentage: Percent = Percent::from_percent(50);
	pub const Period: u64 = 1;
	pub const Offset: u64 = 0;
	pub const RefreshDelay: Permill = Permill::from_percent(90);
	pub const TimeToRestart: u64 = 3;
}

impl pallet_session::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type ValidatorId = AccountId;
	type ValidatorIdOf = ConvertInto;
	type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type SessionManager = MockSessionManager;
	type SessionHandler = <MockSessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = MockSessionKeys;
	type WeightInfo = ();
}

impl pallet_dkg_metadata::Config for Test {
	type DKGId = DKGId;
	type RuntimeEvent = RuntimeEvent;
	type OnAuthoritySetChangeHandler = ();
	type OnDKGPublicKeyChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type RefreshDelay = RefreshDelay;
	type KeygenJailSentence = Period;
	type SigningJailSentence = Period;
	type SessionPeriod = Period;
	type DecayPercentage = DecayPercentage;
	type Reputation = u128;
	type UnsignedInterval = frame_support::traits::ConstU64<0>;
	type UnsignedPriority = frame_support::traits::ConstU64<{ 1 << 20 }>;
	type AuthorityIdOf = pallet_dkg_metadata::AuthorityIdOf<Self>;
	type ProposalHandler = ();
	type WeightInfo = ();
}

const PHRASE: &str = "news slush supreme milk chapter athlete soap sausage put clutch what kitten";

#[allow(dead_code)]
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = system::GenesisConfig::default().build_storage::<Test>().unwrap();
	t.into()
}

#[allow(dead_code)]
pub fn new_test_ext_benchmarks() -> sp_io::TestExternalities {
	let mut t = system::GenesisConfig::default().build_storage::<Test>().unwrap();
	let mut t_ext = sp_io::TestExternalities::from(t);
	let keystore = KeyStore::new();
	t_ext.register_extension(KeystoreExt(Arc::new(keystore)));
	t_ext
}

pub fn execute_test_with<R>(execute: impl FnOnce() -> R) -> R {
	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let keystore = KeyStore::new();
	let (pool, _pool_state) = testing::TestTransactionPoolExt::new();

	let mock_dkg_pub_key = SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	SyncCryptoStore::sr25519_generate_new(
		&keystore,
		dkg_runtime_primitives::offchain::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();
	let mut t = new_test_ext();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(KeystoreExt(Arc::new(keystore)));
	t.register_extension(TransactionPoolExt::new(pool));

	t.execute_with(|| {
		pallet_dkg_metadata::DKGPublicKey::<Test>::put((0, mock_dkg_pub_key.encode()));
		execute()
	})
}

pub fn mock_eth_tx_eip2930(nonce: u8) -> EIP2930Transaction {
	EIP2930Transaction {
		chain_id: 0,
		nonce: U256::from(nonce),
		gas_price: U256::from(0u8),
		gas_limit: U256::from(0u8),
		action: TransactionAction::Create,
		value: U256::from(0u8),
		input: Vec::<u8>::new(),
		access_list: Vec::new(),
		odd_y_parity: false,
		r: H256::from([0u8; 32]),
		s: H256::from([0u8; 32]),
	}
}

pub fn mock_sign_msg(
	msg: &[u8; 32],
) -> Result<std::option::Option<sp_core::ecdsa::Signature>, sp_keystore::Error> {
	let keystore = KeyStore::new();
	let (_pool, _pool_state) = testing::TestTransactionPoolExt::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let pub_key =
		*SyncCryptoStore::ecdsa_public_keys(&keystore, dkg_runtime_primitives::crypto::Public::ID)
			.get(0)
			.unwrap();

	keystore.ecdsa_sign_prehashed(dkg_runtime_primitives::crypto::Public::ID, &pub_key, msg)
}

pub fn mock_signed_proposal(eth_tx: TransactionV2) -> Proposal {
	let eth_tx_ser = eth_tx.encode();

	let hash = keccak_256(&eth_tx_ser);
	let sig = mock_sign_msg(&hash).unwrap().unwrap();

	let mut sig_vec: Vec<u8> = Vec::new();
	sig_vec.extend_from_slice(&sig.0);

	Proposal::Signed { kind: ProposalKind::EVM, data: eth_tx_ser, signature: sig_vec }
}
