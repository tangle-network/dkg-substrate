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
#![allow(clippy::unwrap_used)]
use crate as pallet_dkg_proposal_handler;
use codec::{Decode, Encode, MaxEncodedLen};
pub use dkg_runtime_primitives::{
	crypto::AuthorityId as DKGId, ConsensusLog, MaxAuthorities, MaxKeyLength, MaxReporters,
	MaxSignatureLength, DKG_ENGINE_ID,
};
use frame_support::{
	parameter_types,
	traits::{ConstBool, Everything},
	BoundedVec, PalletId,
};
use frame_system as system;
use frame_system::EnsureRoot;
use pallet_dkg_proposals::DKGEcdsaToEthereumAddress;
use pallet_session::historical as pallet_session_historical;
use sp_core::{sr25519::Signature, H256};
use sp_runtime::{
	impl_opaque_keys,
	testing::TestXt,
	traits::{
		BlakeTwo256, ConvertInto, Extrinsic as ExtrinsicT, IdentifyAccount, IdentityLookup,
		OpaqueKeys, Verify,
	},
	BuildStorage, Percent, Permill,
};
use sp_staking::{
	offence::{OffenceError, ReportOffence},
	SessionIndex,
};

use sp_core::offchain::{testing, OffchainDbExt, OffchainWorkerExt, TransactionPoolExt};

use sp_keystore::{testing::MemoryKeystore, Keystore, KeystoreExt};

use sp_runtime::RuntimeAppPublic;

use dkg_runtime_primitives::{
	keccak_256, MaxProposalLength, MaxResources, MaxVotes, TransactionV2, TypedChainId,
};

use crate::SignedProposalBatchOf;
use dkg_runtime_primitives::{EIP2930Transaction, TransactionAction, U256};
use std::sync::Arc;
use webb_proposals::{Proposal, ProposalKind};

impl_opaque_keys! {
	pub struct MockSessionKeys {
		pub dummy: pallet_dkg_metadata::Pallet<Test>,
	}
}

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Session: pallet_session,
		DKG: pallet_dkg_metadata,
		Balances: pallet_balances,
		DKGProposals: pallet_dkg_proposals,
		DKGProposalHandler: pallet_dkg_proposal_handler,
		Aura: pallet_aura,
		Historical: pallet_session_historical,
	}
);

impl pallet_aura::Config for Test {
	type AuthorityId = sp_consensus_aura::sr25519::AuthorityId;
	type DisabledValidators = ();
	type MaxAuthorities = MaxAuthorities;
	type AllowMultipleBlocksPerSlot = ConstBool<false>;
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
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = frame_system::mocking::MockBlock<Test>;
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
	type FreezeIdentifier = ();
	type RuntimeHoldReason = RuntimeHoldReason;
	type MaxHolds = ();
	type MaxFreezes = ();
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

type IdentificationTuple = (AccountId, AccountId);
type Offence = crate::DKGMisbehaviourOffence<IdentificationTuple>;

parameter_types! {
	pub static Offences: Vec<(Vec<AccountId>, Offence)> = vec![];
}

/// A mock offence report handler.
pub struct OffenceHandler;
impl ReportOffence<AccountId, IdentificationTuple, Offence> for OffenceHandler {
	fn report_offence(reporters: Vec<AccountId>, offence: Offence) -> Result<(), OffenceError> {
		Offences::mutate(|l| l.push((reporters, offence)));
		Ok(())
	}

	fn is_known_offence(_offenders: &[IdentificationTuple], _time_slot: &SessionIndex) -> bool {
		false
	}
}

parameter_types! {
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxProposers : u32 = 100;
	#[derive(Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd)]
	pub const MaxProposalsPerBatch : u32 = 10;
}

impl pallet_dkg_proposal_handler::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type UnsignedProposalExpiry = frame_support::traits::ConstU64<10>;
	type SignedProposalHandler = ();
	type BatchId = u32;
	type MaxProposalsPerBatch = MaxProposalsPerBatch;
	type ForceOrigin = EnsureRoot<Self::AccountId>;
	type ValidatorSet = Historical;
	type ReportOffences = OffenceHandler;
	type WeightInfo = ();
}

impl pallet_dkg_proposals::Config for Test {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAuthorityToMerkleLeaf = DKGEcdsaToEthereumAddress;
	type DKGId = DKGId;
	type ChainIdentifier = ChainIdentifier;
	type MaxProposalLength = MaxProposalLength;
	type RuntimeEvent = RuntimeEvent;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
	type Period = Period;
	type MaxVotes = MaxVotes;
	type MaxResources = MaxResources;
	type MaxProposers = MaxProposers;
	type VotingKeySize = MaxKeyLength;
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

impl pallet_session::historical::Config for Test {
	type FullIdentification = AccountId;
	type FullIdentificationOf = ConvertInto;
}

parameter_types! {
	#[derive(Default, Clone, Encode, Decode, Debug, Eq, PartialEq, scale_info::TypeInfo, Ord, PartialOrd, MaxEncodedLen)]
	pub const VoteLength: u32 = 64;
}

impl pallet_dkg_metadata::Config for Test {
	type DKGId = DKGId;
	type RuntimeEvent = RuntimeEvent;
	type OnAuthoritySetChangeHandler = ();
	type OnDKGPublicKeyChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type KeygenJailSentence = Period;
	type SigningJailSentence = Period;
	type ForceOrigin = EnsureRoot<Self::AccountId>;
	type SessionPeriod = Period;
	type DecayPercentage = DecayPercentage;
	type Reputation = u128;
	type UnsignedInterval = frame_support::traits::ConstU64<0>;
	type UnsignedPriority = frame_support::traits::ConstU64<1000>;
	type AuthorityIdOf = pallet_dkg_metadata::AuthorityIdOf<Self>;
	type ProposalHandler = ();
	type MaxKeyLength = MaxKeyLength;
	type MaxSignatureLength = MaxSignatureLength;
	type MaxReporters = MaxReporters;
	type MaxAuthorities = MaxAuthorities;
	type VoteLength = VoteLength;
	type MaxProposalLength = MaxProposalLength;
	type WeightInfo = ();
}

const PHRASE: &str = "news slush supreme milk chapter athlete soap sausage put clutch what kitten";

#[allow(dead_code)]
pub fn new_test_ext() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	t.into()
}

#[allow(dead_code)]
pub fn new_test_ext_benchmarks() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	let mut t_ext = sp_io::TestExternalities::from(t);
	let keystore = MemoryKeystore::new();
	t_ext.register_extension(KeystoreExt(Arc::new(keystore)));
	t_ext
}

pub fn execute_test_with<R>(execute: impl FnOnce() -> R) -> R {
	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let keystore = MemoryKeystore::new();
	let (pool, _pool_state) = testing::TestTransactionPoolExt::new();

	let mock_dkg_pub_key = Keystore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	Keystore::sr25519_generate_new(
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

	let bounded_key: BoundedVec<_, _> = mock_dkg_pub_key.encode().try_into().unwrap();
	t.execute_with(|| {
		pallet_dkg_metadata::DKGPublicKey::<Test>::put((0, bounded_key));
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
	let keystore = MemoryKeystore::new();
	let (_pool, _pool_state) = testing::TestTransactionPoolExt::new();

	Keystore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let pub_key =
		*Keystore::ecdsa_public_keys(&keystore, dkg_runtime_primitives::crypto::Public::ID)
			.get(0)
			.unwrap();

	keystore.ecdsa_sign_prehashed(dkg_runtime_primitives::crypto::Public::ID, &pub_key, msg)
}

pub fn mock_signed_proposal_batch(eth_tx: TransactionV2) -> SignedProposalBatchOf<Test> {
	let eth_tx_ser = eth_tx.encode();

	let hash = keccak_256(&eth_tx_ser);
	let sig = mock_sign_msg(&hash).unwrap().unwrap();

	let mut sig_vec: Vec<u8> = Vec::new();
	sig_vec.extend_from_slice(&sig.0);

	let unsigned_proposal =
		Proposal::Unsigned { kind: ProposalKind::EVM, data: eth_tx.encode().try_into().unwrap() };

	SignedProposalBatchOf::<Test> {
		proposals: vec![unsigned_proposal].try_into().unwrap(),
		batch_id: 0_u32,
		signature: sig_vec.try_into().unwrap(),
	}
}
