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
#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

use codec::Encode;
use dkg_runtime_primitives::{TypedChainId, UnsignedProposal};
use frame_election_provider_support::{onchain, SequentialPhragmen, VoteWeight};
use frame_support::{
	traits::{ConstU16, ConstU32, Everything, U128CurrencyToVote},
	weights::ConstantMultiplier,
};
#[cfg(any(feature = "std", test))]
pub use frame_system::Call as SystemCall;
use frame_system::{
	limits::{BlockLength, BlockWeights},
	EnsureRoot,
};
use pallet_dkg_proposals::DKGEcdsaToEthereum;
use pallet_election_provider_multi_phase::SolutionAccuracyOf;
use pallet_grandpa::{
	fg_primitives, AuthorityId as GrandpaId, AuthorityList as GrandpaAuthorityList,
};
use pallet_session::historical as pallet_session_historical;
#[cfg(any(feature = "std", test))]
pub use pallet_staking::StakerStatus;
pub use pallet_transaction_payment::{CurrencyAdapter, Multiplier, TargetedFeeAdjustment};
use sp_api::impl_runtime_apis;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{crypto::KeyTypeId, OpaqueMetadata};
use sp_runtime::{
	create_runtime_str,
	curve::PiecewiseLinear,
	generic, impl_opaque_keys,
	traits::{
		self, AccountIdLookup, BlakeTwo256, Block as BlockT, IdentifyAccount, NumberFor,
		OpaqueKeys, StaticLookup, Verify,
	},
	transaction_validity::{TransactionPriority, TransactionSource, TransactionValidity},
	ApplyExtrinsicResult, FixedPointNumber, MultiSignature, Percent, Perquintill,
	SaturatedConversion,
};
use sp_std::{convert::TryInto, prelude::*};
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

// A few exports that help ease life for downstream crates.
pub use frame_support::{
	construct_runtime, parameter_types,
	traits::{KeyOwnerProofSystem, Randomness, StorageInfo},
	weights::{
		constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_PER_SECOND},
		DispatchClass, IdentityFee, Weight,
	},
	PalletId, StorageValue,
};
pub use pallet_balances::Call as BalancesCall;
use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
pub use pallet_timestamp::Call as TimestampCall;
use sp_runtime::generic::Era;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use sp_runtime::{Perbill, Permill};

pub use dkg_runtime_primitives::crypto::AuthorityId as DKGId;

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
pub type Signature = MultiSignature;

/// Some way of identifying an account on the chain. We intentionally make it equivalent
/// to the public key of our transaction signing scheme.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;
/// Resource ID type
pub type ResourceId = [u8; 32];
/// Reputation type
pub type Reputation = u128;

pub type CheckedExtrinsic = generic::CheckedExtrinsic<AccountId, Call, SignedExtra>;

pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;

/// Index of a transaction in the chain.
pub type Index = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

pub type AccountIndex = u32;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
	use super::*;

	pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

	/// Opaque block header type.
	pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// Opaque block type.
	pub type Block = generic::Block<Header, UncheckedExtrinsic>;
	/// Opaque block identifier type.
	pub type BlockId = generic::BlockId<Block>;

	impl_opaque_keys! {
	  pub struct SessionKeys {
		pub aura: Aura,
		pub grandpa: Grandpa,
		pub im_online: ImOnline,
		pub dkg: DKG,
	  }
	}
}

#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: create_runtime_str!("dkg-standalone-node"),
	impl_name: create_runtime_str!("dkg-standalone-node"),
	authoring_version: 1,
	spec_version: 15,
	impl_version: 0,
	apis: RUNTIME_API_VERSIONS,
	transaction_version: 1,
	state_version: 1,
};

pub mod constants;
pub use constants::{currency::*, time::*};

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
	NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
  pub const BlockHashCount: BlockNumber = 2400;
  pub const Version: RuntimeVersion = VERSION;
  pub RuntimeBlockLength: BlockLength =
	BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
  pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
	.base_block(BlockExecutionWeight::get())
	.for_class(DispatchClass::all(), |weights| {
	  weights.base_extrinsic = ExtrinsicBaseWeight::get();
	})
	.for_class(DispatchClass::Normal, |weights| {
	  weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
	})
	.for_class(DispatchClass::Operational, |weights| {
	  weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
	  // Operational transactions have some extra reserved space, so that they
	  // are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
	  weights.reserved = Some(
		MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT
	  );
	})
	.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
	.build_or_panic();
  pub const SS58Prefix: u16 = 42;
}

impl frame_system::Config for Runtime {
	type AccountData = pallet_balances::AccountData<Balance>;
	type AccountId = AccountId;
	type BaseCallFilter = Everything;
	type BlockHashCount = BlockHashCount;
	type BlockLength = RuntimeBlockLength;
	type BlockNumber = BlockNumber;
	type BlockWeights = RuntimeBlockWeights;
	type Call = Call;
	type DbWeight = RocksDbWeight;
	type Event = Event;
	type Hash = Hash;
	type Hashing = BlakeTwo256;
	type Header = generic::Header<BlockNumber, BlakeTwo256>;
	type Index = Index;
	type Lookup = Indices;
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type OnKilledAccount = ();
	type OnNewAccount = ();
	type OnSetCode = ();
	type Origin = Origin;
	type PalletInfo = PalletInfo;
	type SS58Prefix = SS58Prefix;
	type SystemWeightInfo = frame_system::weights::SubstrateWeight<Runtime>;
	type Version = Version;
}

parameter_types! {
  pub const IndexDeposit: Balance = DOLLARS;
}

parameter_types! {
	pub const BasicDeposit: Balance = deposit(1, 258);
	pub const FieldDeposit: Balance = deposit(0, 66);
	pub const SubAccountDeposit: Balance = deposit(1, 53);
	pub const MaxSubAccounts: u32 = 100;
	pub const MaxAdditionalFields: u32 = 100;
	pub const MaxRegistrars: u32 = 20;
}

impl pallet_identity::Config for Runtime {
	type Event = Event;
	type Currency = Balances;
	type BasicDeposit = BasicDeposit;
	type FieldDeposit = FieldDeposit;
	type SubAccountDeposit = SubAccountDeposit;
	type MaxSubAccounts = MaxSubAccounts;
	type MaxAdditionalFields = MaxAdditionalFields;
	type MaxRegistrars = MaxRegistrars;
	type Slashed = ();
	type ForceOrigin = EnsureRoot<Self::AccountId>;
	type RegistrarOrigin = EnsureRoot<Self::AccountId>;
	type WeightInfo = ();
}
impl pallet_indices::Config for Runtime {
	type AccountIndex = AccountIndex;
	type Currency = Balances;
	type Deposit = IndexDeposit;
	type Event = Event;
	type WeightInfo = pallet_indices::weights::SubstrateWeight<Runtime>;
}

impl pallet_randomness_collective_flip::Config for Runtime {}

parameter_types! {
  pub const MaxAuthorities: u32 = 1_000;
}

impl pallet_aura::Config for Runtime {
	type AuthorityId = AuraId;
	type DisabledValidators = ();
	type MaxAuthorities = MaxAuthorities;
}

impl pallet_grandpa::Config for Runtime {
	type Event = Event;
	type Call = Call;
	type MaxAuthorities = MaxAuthorities;

	type KeyOwnerProofSystem = ();

	type KeyOwnerProof =
		<Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(KeyTypeId, GrandpaId)>>::Proof;

	type KeyOwnerIdentification = <Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(
		KeyTypeId,
		GrandpaId,
	)>>::IdentificationTuple;

	type HandleEquivocation = ();

	type WeightInfo = ();
}

parameter_types! {
  pub const MinimumPeriod: u64 = SLOT_DURATION / 2;
}

impl pallet_timestamp::Config for Runtime {
	/// A timestamp: milliseconds since the unix epoch.
	type Moment = u64;
	#[cfg(feature = "manual-seal")]
	type OnTimestampSet = ();
	#[cfg(not(feature = "manual-seal"))]
	type OnTimestampSet = Aura;
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

#[cfg(feature = "integration-tests")]
parameter_types! {
  // How often we trigger a new session.
  // during integration tests, we use manual sessions.
  pub const Period: BlockNumber = 1 * HOURS;
  pub const Offset: BlockNumber = 0;
}

#[cfg(not(feature = "integration-tests"))]
parameter_types! {
  // How often we trigger a new session.
  pub const Period: BlockNumber = 1 * HOURS;
  pub const Offset: BlockNumber = 0;
}

impl pallet_session::Config for Runtime {
	type Event = Event;
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_staking::StashOf<Self>;
	type ShouldEndSession = pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime>;
	type NextSessionRotation = pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime>;
	type SessionManager = Staking;
	type SessionHandler = <opaque::SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = opaque::SessionKeys;
	type WeightInfo = pallet_session::weights::SubstrateWeight<Runtime>;
}

impl pallet_session::historical::Config for Runtime {
	type FullIdentification = pallet_staking::Exposure<AccountId, Balance>;
	type FullIdentificationOf = pallet_staking::ExposureOf<Runtime>;
}

pallet_staking_reward_curve::build! {
  const REWARD_CURVE: PiecewiseLinear<'static> = curve!(
	min_inflation: 0_025_000,
	max_inflation: 0_100_000,
	ideal_stake: 0_500_000,
	falloff: 0_050_000,
	max_piece_count: 40,
	test_precision: 0_005_000,
  );
}

parameter_types! {
  pub const SessionsPerEra: sp_staking::SessionIndex = 6;
  pub const BondingDuration: sp_staking::EraIndex = 24 * 28;
  pub const SlashDeferDuration: sp_staking::EraIndex = 24 * 7; // 1/4 the bonding duration.
  pub const RewardCurve: &'static PiecewiseLinear<'static> = &REWARD_CURVE;
  pub const MaxNominatorRewardedPerValidator: u32 = 256;
  pub const OffendingValidatorsThreshold: Perbill = Perbill::from_percent(17);
  pub OffchainRepeat: BlockNumber = 5;
}

pub struct StakingBenchmarkingConfig;
impl pallet_staking::BenchmarkingConfig for StakingBenchmarkingConfig {
	type MaxNominators = ConstU32<1000>;
	type MaxValidators = ConstU32<1000>;
}

impl pallet_staking::Config for Runtime {
	type MaxNominations = MaxNominations;
	type Currency = Balances;
	type CurrencyBalance = Balance;
	type UnixTime = Timestamp;
	type CurrencyToVote = U128CurrencyToVote;
	type RewardRemainder = ();
	type Event = Event;
	type Slash = ();
	type Reward = ();
	type SessionsPerEra = SessionsPerEra;
	type BondingDuration = BondingDuration;
	type SlashDeferDuration = SlashDeferDuration;
	/// A super-majority of the council can cancel the slash.
	type SlashCancelOrigin = EnsureRoot<AccountId>;
	type SessionInterface = Self;
	type EraPayout = pallet_staking::ConvertCurve<RewardCurve>;
	type NextNewSession = Session;
	type MaxNominatorRewardedPerValidator = MaxNominatorRewardedPerValidator;
	type OffendingValidatorsThreshold = OffendingValidatorsThreshold;
	type ElectionProvider = ElectionProviderMultiPhase;
	type GenesisElectionProvider = onchain::UnboundedExecution<OnChainSeqPhragmen>;
	type VoterList = BagsList;
	type MaxUnlockingChunks = ConstU32<32>;
	type OnStakerSlash = NominationPools;
	type WeightInfo = pallet_staking::weights::SubstrateWeight<Runtime>;
	type BenchmarkingConfig = StakingBenchmarkingConfig;
}

parameter_types! {
  // phase durations. 1/4 of the last session for each.
  pub const SignedPhase: u32 = EPOCH_DURATION_IN_BLOCKS as u32 / 4;
  pub const UnsignedPhase: u32 = EPOCH_DURATION_IN_BLOCKS as u32 / 4;

  // signed config
  pub const SignedRewardBase: Balance = DOLLARS;
  pub const SignedDepositBase: Balance = DOLLARS;
  pub const SignedDepositByte: Balance = CENTS;
  pub const StakingUnsignedPriority: TransactionPriority = TransactionPriority::max_value() / 2;
  pub BetterUnsignedThreshold: Perbill = Perbill::from_rational(1u32, 10_000);

  // miner configs
  pub const MultiPhaseUnsignedPriority: TransactionPriority = StakingUnsignedPriority::get() - 1u64;
  pub MinerMaxWeight: Weight = RuntimeBlockWeights::get()
	.get(DispatchClass::Normal)
	.max_extrinsic.expect("Normal extrinsics have a weight limit configured; qed")
	.saturating_sub(BlockExecutionWeight::get());
  // Solution can occupy 90% of normal block size
  pub MinerMaxLength: u32 = Perbill::from_rational(9u32, 10) *
	*RuntimeBlockLength::get()
	.max
	.get(DispatchClass::Normal);
}

frame_election_provider_support::generate_solution_type!(
  #[compact]
  pub struct NposSolution16::<
	VoterIndex = u32,
	TargetIndex = u16,
	Accuracy = sp_runtime::PerU16,
	MaxVoters = MaxElectingVoters,
  >(16)
);

parameter_types! {
  pub MaxNominations: u32 = <NposSolution16 as frame_election_provider_support::NposSolution>::LIMIT as u32;
  pub MaxElectingVoters: u32 = 10_000;
}

/// The numbers configured here could always be more than the the maximum limits of staking pallet
/// to ensure election snapshot will not run out of memory. For now, we set them to smaller values
/// since the staking is bounded and the weight pipeline takes hours for this single pallet.
pub struct ElectionProviderBenchmarkConfig;
impl pallet_election_provider_multi_phase::BenchmarkingConfig for ElectionProviderBenchmarkConfig {
	const VOTERS: [u32; 2] = [1000, 2000];
	const TARGETS: [u32; 2] = [500, 1000];
	const ACTIVE_VOTERS: [u32; 2] = [500, 800];
	const DESIRED_TARGETS: [u32; 2] = [200, 400];
	const SNAPSHOT_MAXIMUM_VOTERS: u32 = 1000;
	const MINER_MAXIMUM_VOTERS: u32 = 1000;
	const MAXIMUM_TARGETS: u32 = 300;
}

/// Maximum number of iterations for balancing that will be executed in the embedded OCW
/// miner of election provider multi phase.
pub const MINER_MAX_ITERATIONS: u32 = 10;

pub struct OnChainSeqPhragmen;
impl onchain::Config for OnChainSeqPhragmen {
	type System = Runtime;
	type Solver = SequentialPhragmen<
		AccountId,
		pallet_election_provider_multi_phase::SolutionAccuracyOf<Runtime>,
	>;
	type DataProvider = <Runtime as pallet_election_provider_multi_phase::Config>::DataProvider;
	type WeightInfo = frame_election_provider_support::weights::SubstrateWeight<Runtime>;
}

impl onchain::BoundedConfig for OnChainSeqPhragmen {
	type VotersBound = MaxElectingVoters;
	type TargetsBound = ConstU32<2_000>;
}

pub struct WebbMinerConfig;
impl pallet_election_provider_multi_phase::MinerConfig for WebbMinerConfig {
	type AccountId = AccountId;
	type MaxLength = MinerMaxLength;
	type MaxWeight = MinerMaxWeight;
	type MaxVotesPerVoter = MaxNominations;
	type Solution = NposSolution16;

	#[allow(unused)]
	fn solution_weight(v: u32, t: u32, a: u32, d: u32) -> Weight {
		Weight::from_ref_time(0)
	}
}

impl pallet_election_provider_multi_phase::Config for Runtime {
	type Event = Event;
	type Currency = Balances;
	type EstimateCallFee = TransactionPayment;
	type SignedPhase = SignedPhase;
	type UnsignedPhase = UnsignedPhase;
	type BetterUnsignedThreshold = BetterUnsignedThreshold;
	type BetterSignedThreshold = ();
	type MinerConfig = WebbMinerConfig;
	type OffchainRepeat = OffchainRepeat;
	type MinerTxPriority = MultiPhaseUnsignedPriority;
	type SignedMaxSubmissions = ConstU32<10>;
	type SignedRewardBase = SignedRewardBase;
	type SignedDepositBase = SignedDepositBase;
	type SignedDepositByte = SignedDepositByte;
	type SignedMaxRefunds = ConstU32<3>;
	type SignedDepositWeight = ();
	type SignedMaxWeight = MinerMaxWeight;
	type SlashHandler = (); // burn slashes
	type RewardHandler = (); // nothing to do upon rewards
	type DataProvider = Staking;
	type Fallback = onchain::BoundedExecution<OnChainSeqPhragmen>;
	type GovernanceFallback = onchain::BoundedExecution<OnChainSeqPhragmen>;
	type Solver = SequentialPhragmen<AccountId, SolutionAccuracyOf<Self>, ()>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type MaxElectableTargets = ConstU16<{ u16::MAX }>;
	type MaxElectingVoters = MaxElectingVoters;
	type BenchmarkingConfig = ElectionProviderBenchmarkConfig;
	type WeightInfo = pallet_election_provider_multi_phase::weights::SubstrateWeight<Self>;
}

impl pallet_bags_list::Config for Runtime {
	type Event = Event;
	type ScoreProvider = Staking;
	type WeightInfo = pallet_bags_list::weights::SubstrateWeight<Runtime>;
	type BagThresholds = ();
	type Score = VoteWeight;
}

parameter_types! {
  pub const PostUnbondPoolsWindow: u32 = 4;
  pub const NominationPoolsPalletId: PalletId = PalletId(*b"py/nopls");
  pub const MaxPointsToBalance: u8 = 10;
}

use sp_runtime::traits::Convert;
pub struct BalanceToU256;
impl Convert<Balance, sp_core::U256> for BalanceToU256 {
	fn convert(balance: Balance) -> sp_core::U256 {
		sp_core::U256::from(balance)
	}
}
pub struct U256ToBalance;
impl Convert<sp_core::U256, Balance> for U256ToBalance {
	fn convert(n: sp_core::U256) -> Balance {
		n.try_into().unwrap_or(Balance::max_value())
	}
}

impl pallet_nomination_pools::Config for Runtime {
	type WeightInfo = ();
	type Event = Event;
	type Currency = Balances;
	type CurrencyBalance = Balance;
	type RewardCounter = sp_runtime::FixedU128;
	type BalanceToU256 = BalanceToU256;
	type U256ToBalance = U256ToBalance;
	type StakingInterface = pallet_staking::Pallet<Self>;
	type PostUnbondingPoolsWindow = PostUnbondPoolsWindow;
	type MaxMetadataLen = ConstU32<256>;
	type MaxUnbonding = ConstU32<8>;
	type PalletId = NominationPoolsPalletId;
	type MaxPointsToBalance = MaxPointsToBalance;
}

parameter_types! {
  pub const ExistentialDeposit: u128 = EXISTENTIAL_DEPOSIT;
  pub const MaxLocks: u32 = 50;
}

impl pallet_balances::Config for Runtime {
	type MaxLocks = MaxLocks;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	/// The type for recording an account's balance.
	type Balance = Balance;
	/// The ubiquitous event type.
	type Event = Event;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
  pub const TransactionByteFee: Balance = 10 * MILLICENTS;
  pub const OperationalFeeMultiplier: u8 = 5;
  pub const TargetBlockFullness: Perquintill = Perquintill::from_percent(25);
  pub AdjustmentVariable: Multiplier = Multiplier::saturating_from_rational(1, 100_000);
  pub MinimumMultiplier: Multiplier = Multiplier::saturating_from_rational(1, 1_000_000_000u128);
}

impl pallet_transaction_payment::Config for Runtime {
	type Event = Event;
	type OnChargeTransaction = CurrencyAdapter<Balances, ()>;
	type OperationalFeeMultiplier = OperationalFeeMultiplier;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = ConstantMultiplier<Balance, TransactionByteFee>;
	type FeeMultiplierUpdate =
		TargetedFeeAdjustment<Self, TargetBlockFullness, AdjustmentVariable, MinimumMultiplier>;
}

impl pallet_sudo::Config for Runtime {
	type Event = Event;
	type Call = Call;
}

parameter_types! {
  pub const DecayPercentage: Percent = Percent::from_percent(50);
}

impl pallet_dkg_metadata::Config for Runtime {
	type DKGId = DKGId;
	type Event = Event;
	type OnAuthoritySetChangeHandler = DKGProposals;
	type OnDKGPublicKeyChangeHandler = ();
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type NextSessionRotation = pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime>;
	type RefreshDelay = RefreshDelay;
	type KeygenJailSentence = Period;
	type SigningJailSentence = Period;
	type DecayPercentage = DecayPercentage;
	type Reputation = Reputation;
	type AuthorityIdOf = pallet_dkg_metadata::AuthorityIdOf<Self>;
	type ProposalHandler = DKGProposalHandler;
	type WeightInfo = pallet_dkg_metadata::weights::WebbWeight<Runtime>;
}

parameter_types! {
  pub const ChainIdentifier: TypedChainId = TypedChainId::Substrate(5);
  pub const ProposalLifetime: BlockNumber = HOURS / 5;
  pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
  pub const RefreshDelay: Permill = Permill::from_percent(50);
  pub const TimeToRestart: BlockNumber = 3;
  // 1 hr considering block time of 12sec
  pub const UnsignedProposalExpiry : BlockNumber = 300;
}

impl pallet_dkg_proposal_handler::Config for Runtime {
	type Event = Event;
	type OffChainAuthId = dkg_runtime_primitives::offchain::crypto::OffchainAuthId;
	type MaxSubmissionsPerBatch = frame_support::traits::ConstU16<100>;
	type UnsignedProposalExpiry = UnsignedProposalExpiry;
	type SignedProposalHandler = ();
	type WeightInfo = pallet_dkg_proposal_handler::weights::WebbWeight<Runtime>;
}

impl pallet_dkg_proposals::Config for Runtime {
	type AdminOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type DKGAuthorityToMerkleLeaf = DKGEcdsaToEthereum;
	type DKGId = DKGId;
	type ChainIdentifier = ChainIdentifier;
	type Event = Event;
	type NextSessionRotation = pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime>;
	type Proposal = Vec<u8>;
	type ProposalLifetime = ProposalLifetime;
	type ProposalHandler = DKGProposalHandler;
	type Period = Period;
	type WeightInfo = pallet_dkg_proposals::WebbWeight<Runtime>;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Runtime
where
	Call: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: Call,
		public: <Signature as traits::Verify>::Signer,
		account: AccountId,
		nonce: Index,
	) -> Option<(Call, <UncheckedExtrinsic as traits::Extrinsic>::SignaturePayload)> {
		let tip = 0;
		// take the biggest period possible.
		let period =
			BlockHashCount::get().checked_next_power_of_two().map(|c| c / 2).unwrap_or(2) as u64;
		let current_block = System::block_number()
			.saturated_into::<u64>()
			// The `System::block_number` is initialized with `n+1`,
			// so the actual block number is `n`.
			.saturating_sub(1);
		let era = Era::mortal(period, current_block);
		let extra = (
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(era),
			frame_system::CheckNonce::<Runtime>::from(nonce),
			frame_system::CheckWeight::<Runtime>::new(),
			pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(tip),
		);

		let raw_payload = SignedPayload::new(call, extra)
			.map_err(|e| {
				frame_support::log::warn!("Unable to create signed payload: {:?}", e);
			})
			.ok()?;
		let signature = raw_payload.using_encoded(|payload| C::sign(payload, public))?;
		let address = AccountIdLookup::<AccountId, Index>::unlookup(account);
		let (call, extra, _) = raw_payload.deconstruct();
		Some((call, (address, signature, extra)))
	}
}

impl frame_system::offchain::SigningTypes for Runtime {
	type Public = <Signature as sp_runtime::traits::Verify>::Signer;
	type Signature = Signature;
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime
where
	Call: From<C>,
{
	type OverarchingCall = Call;
	type Extrinsic = UncheckedExtrinsic;
}

parameter_types! {
	pub const MaxResources: u32 = 32;
}

type BridgeRegistryInstance = pallet_bridge_registry::Instance1;
impl pallet_bridge_registry::Config<BridgeRegistryInstance> for Runtime {
	type Event = Event;
	type BridgeIndex = u32;
	type MaxAdditionalFields = MaxAdditionalFields;
	type MaxResources = MaxResources;
	type ForceOrigin = frame_system::EnsureRoot<AccountId>;
	type WeightInfo = ();
}

parameter_types! {
	pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
	pub const MaxKeys: u32 = 10_000;
	pub const MaxPeerInHeartbeats: u32 = 10_000;
	pub const MaxPeerDataEncodingSize: u32 = 1_000;
}

impl pallet_im_online::Config for Runtime {
	type AuthorityId = ImOnlineId;
	type Event = Event;
	type NextSessionRotation = pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime>;
	type ValidatorSet = Historical;
	type ReportUnresponsiveness = ();
	type UnsignedPriority = ImOnlineUnsignedPriority;
	type WeightInfo = pallet_im_online::weights::SubstrateWeight<Runtime>;
	type MaxKeys = MaxKeys;
	type MaxPeerInHeartbeats = MaxPeerInHeartbeats;
	type MaxPeerDataEncodingSize = MaxPeerDataEncodingSize;
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
  pub enum Runtime where
	Block = Block,
	NodeBlock = opaque::Block,
	UncheckedExtrinsic = UncheckedExtrinsic
  {
	System: frame_system,
	Indices: pallet_indices,
	RandomnessCollectiveFlip: pallet_randomness_collective_flip,
	Timestamp: pallet_timestamp,
	Aura: pallet_aura,
	Grandpa: pallet_grandpa,
	Balances: pallet_balances,
	DKG: pallet_dkg_metadata,
	DKGProposals: pallet_dkg_proposals,
	DKGProposalHandler: pallet_dkg_proposal_handler,
	TransactionPayment: pallet_transaction_payment,
	Sudo: pallet_sudo,
	ElectionProviderMultiPhase: pallet_election_provider_multi_phase,
	BagsList: pallet_bags_list,
	NominationPools: pallet_nomination_pools,
	Staking: pallet_staking,
	Session: pallet_session,
	Historical: pallet_session_historical,
	BridgeRegistry: pallet_bridge_registry::<Instance1>,
	Identity: pallet_identity::{Pallet, Call, Storage, Event<T>},
	ImOnline: pallet_im_online,
  }
);

/// The address format for describing accounts.
pub type Address = sp_runtime::MultiAddress<AccountId, Index>;
/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
	frame_system::CheckNonZeroSender<Runtime>,
	frame_system::CheckSpecVersion<Runtime>,
	frame_system::CheckTxVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
);
/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
>;

impl_runtime_apis! {
  impl sp_api::Core<Block> for Runtime {
	fn version() -> RuntimeVersion {
	  VERSION
	}

	fn execute_block(block: Block) {
	  Executive::execute_block(block);
	}

	fn initialize_block(header: &<Block as BlockT>::Header) {
	  Executive::initialize_block(header)
	}
  }

  impl sp_api::Metadata<Block> for Runtime {
	fn metadata() -> OpaqueMetadata {
	  OpaqueMetadata::new(Runtime::metadata().into())
	}
  }

  impl sp_block_builder::BlockBuilder<Block> for Runtime {
	fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
	  Executive::apply_extrinsic(extrinsic)
	}

	fn finalize_block() -> <Block as BlockT>::Header {
	  Executive::finalize_block()
	}

	fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
	  data.create_extrinsics()
	}

	fn check_inherents(
	  block: Block,
	  data: sp_inherents::InherentData,
	) -> sp_inherents::CheckInherentsResult {
	  data.check_extrinsics(&block)
	}
  }

  impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
	fn validate_transaction(
	  source: TransactionSource,
	  tx: <Block as BlockT>::Extrinsic,
	  block_hash: <Block as BlockT>::Hash,
	) -> TransactionValidity {
	  Executive::validate_transaction(source, tx, block_hash)
	}
  }

  impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
	fn offchain_worker(header: &<Block as BlockT>::Header) {
	  Executive::offchain_worker(header)
	}
  }

  impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
	fn slot_duration() -> sp_consensus_aura::SlotDuration {
	  sp_consensus_aura::SlotDuration::from_millis(Aura::slot_duration())
	}

	fn authorities() -> Vec<AuraId> {
	  Aura::authorities().into_inner()
	}
  }

  impl sp_session::SessionKeys<Block> for Runtime {
	fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
	  opaque::SessionKeys::generate(seed)
	}

	fn decode_session_keys(
	  encoded: Vec<u8>,
	) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
	  opaque::SessionKeys::decode_into_raw_public_keys(&encoded)
	}
  }

  impl fg_primitives::GrandpaApi<Block> for Runtime {
	fn grandpa_authorities() -> GrandpaAuthorityList {
	  Grandpa::grandpa_authorities()
	}

	fn current_set_id() -> fg_primitives::SetId {
	  Grandpa::current_set_id()
	}

	fn submit_report_equivocation_unsigned_extrinsic(
	  _equivocation_proof: fg_primitives::EquivocationProof<
		<Block as BlockT>::Hash,
		NumberFor<Block>,
	  >,
	  _key_owner_proof: fg_primitives::OpaqueKeyOwnershipProof,
	) -> Option<()> {
	  None
	}

	fn generate_key_ownership_proof(
	  _set_id: fg_primitives::SetId,
	  _authority_id: GrandpaId,
	) -> Option<fg_primitives::OpaqueKeyOwnershipProof> {
	  // NOTE: this is the only implementation possible since we've
	  // defined our key owner proof type as a bottom type (i.e. a type
	  // with no values).
	  None
	}
  }

  impl dkg_runtime_primitives::DKGApi<Block, DKGId, BlockNumber> for Runtime {
	fn authority_set() -> dkg_runtime_primitives::AuthoritySet<DKGId> {
	  let authorities = DKG::authorities();
	  let authority_set_id = DKG::authority_set_id();

	  dkg_runtime_primitives::AuthoritySet {
		authorities,
		id: authority_set_id
	  }
	}

	fn queued_authority_set() -> dkg_runtime_primitives::AuthoritySet<DKGId> {
	  let queued_authorities = DKG::next_authorities();
	  let queued_authority_set_id = DKG::authority_set_id() + 1u64;

	  dkg_runtime_primitives::AuthoritySet {
		authorities: queued_authorities,
		id: queued_authority_set_id
	  }
	}

	fn signature_threshold() -> u16 {
	  DKG::signature_threshold()
	}

	fn keygen_threshold() -> u16 {
	  DKG::keygen_threshold()
	}

	fn next_signature_threshold() -> u16 {
	  DKG::next_signature_threshold()
	}

	fn next_keygen_threshold() -> u16 {
	  DKG::next_keygen_threshold()
	}

	fn should_refresh(block_number: BlockNumber) -> bool {
	  DKG::should_refresh(block_number)
	}

	fn next_dkg_pub_key() -> Option<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)> {
	  DKG::next_dkg_public_key()
	}

	fn next_pub_key_sig() -> Option<Vec<u8>> {
	  DKG::next_public_key_signature()
	}

	fn dkg_pub_key() -> (dkg_runtime_primitives::AuthoritySetId, Vec<u8>) {
	  DKG::dkg_public_key()
	}

	fn get_best_authorities() -> Vec<(u16, DKGId)> {
	  DKG::best_authorities()
	}

	fn get_next_best_authorities() -> Vec<(u16, DKGId)> {
	  DKG::next_best_authorities()
	}

	fn get_current_session_progress(block_number: BlockNumber) -> Option<Permill> {
		use frame_support::traits::EstimateNextSessionRotation;
		<pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime> as EstimateNextSessionRotation<BlockNumber>>::estimate_current_session_progress(block_number).0
	}

	fn get_unsigned_proposals() -> Vec<UnsignedProposal> {
	  DKGProposalHandler::get_unsigned_proposals()
	}

	fn get_max_extrinsic_delay(block_number: BlockNumber) -> BlockNumber {
	  DKG::max_extrinsic_delay(block_number)
	}

	fn get_authority_accounts() -> (Vec<AccountId>, Vec<AccountId>) {
	  (DKG::current_authorities_accounts(), DKG::next_authorities_accounts())
	}

	fn get_reputations(authorities: Vec<DKGId>) -> Vec<(DKGId, Reputation)> {
	  authorities.iter().map(|a| (a.clone(), DKG::authority_reputations(a))).collect()
	}

	fn get_keygen_jailed(set: Vec<DKGId>) -> Vec<DKGId> {
	  set.iter().filter(|a| pallet_dkg_metadata::JailedKeygenAuthorities::<Runtime>::contains_key(a)).cloned().collect()
	}

	fn get_signing_jailed(set: Vec<DKGId>) -> Vec<DKGId> {
	  set.iter().filter(|a| pallet_dkg_metadata::JailedSigningAuthorities::<Runtime>::contains_key(a)).cloned().collect()
	}

	fn refresh_nonce() -> u32 {
	  DKG::refresh_nonce()
	}
  }

  impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
	fn account_nonce(account: AccountId) -> Index {
	  System::account_nonce(account)
	}
  }

  impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
	fn query_info(
	  uxt: <Block as BlockT>::Extrinsic,
	  len: u32,
	) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
	  TransactionPayment::query_info(uxt, len)
	}
	fn query_fee_details(
	  uxt: <Block as BlockT>::Extrinsic,
	  len: u32,
	) -> pallet_transaction_payment::FeeDetails<Balance> {
	  TransactionPayment::query_fee_details(uxt, len)
	}
  }

  #[cfg(feature = "runtime-benchmarks")]
  impl frame_benchmarking::Benchmark<Block> for Runtime {
	fn benchmark_metadata(extra: bool) -> (
	  Vec<frame_benchmarking::BenchmarkList>,
	  Vec<frame_support::traits::StorageInfo>,
	) {
	  use frame_benchmarking::{list_benchmark, Benchmarking, BenchmarkList};
	  use frame_support::traits::StorageInfoTrait;

	  use frame_system_benchmarking::Pallet as SystemBench;

	  let mut list = Vec::<BenchmarkList>::new();

	  list_benchmark!(list, extra, pallet_balances, Balances);
	  list_benchmark!(list, extra, frame_system, SystemBench::<Runtime>);
	  list_benchmark!(list, extra, pallet_timestamp, Timestamp);
	  list_benchmark!(list, extra, pallet_dkg_proposal_handler, DKGProposalHandler);
	  list_benchmark!(list, extra, pallet_dkg_proposals, DKGProposals);
	  list_benchmark!(list, extra, pallet_dkg_metadata, DKG);
	  list_benchmark!(list, extra, pallet_bridge_registry, BridgeRegistry);

	  let storage_info = AllPalletsWithSystem::storage_info();

	  (list, storage_info)
	}

	fn dispatch_benchmark(
	  config: frame_benchmarking::BenchmarkConfig
	) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
	  use frame_benchmarking::{Benchmarking, BenchmarkBatch, add_benchmark, TrackedStorageKey};
	  use frame_support::traits::StorageInfoTrait;

	  use frame_system_benchmarking::Pallet as SystemBench;
	  impl frame_system_benchmarking::Config for Runtime {}

	  let whitelist: Vec<TrackedStorageKey> = vec![
		// Block Number
		hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef702a5c1b19ab7a04f536c519aca4983ac").to_vec().into(),
		// Total Issuance
		hex_literal::hex!("c2261276cc9d1f8598ea4b6a74b15c2f57c875e4cff74148e4628f264b974c80").to_vec().into(),
		// Execution Phase
		hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef7ff553b5a9862a516939d82b3d3d8661a").to_vec().into(),
		// Event Count
		hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef70a98fdbe9ce6c55837576c60c7af3850").to_vec().into(),
		// System Events
		hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7").to_vec().into(),
	  ];

	  let _storage_info = AllPalletsWithSystem::storage_info();

	  let mut batches = Vec::<BenchmarkBatch>::new();
	  let params = (&config, &whitelist);

	  add_benchmark!(params, batches, frame_system, SystemBench::<Runtime>);
	  add_benchmark!(params, batches, pallet_balances, Balances);
	  add_benchmark!(params, batches, pallet_timestamp, Timestamp);
	  add_benchmark!(params, batches, pallet_dkg_proposal_handler, DKGProposalHandler);
	  add_benchmark!(params, batches, pallet_dkg_proposals, DKGProposals);
	  add_benchmark!(params, batches, pallet_dkg_metadata, DKG);
	  add_benchmark!(params, batches, pallet_bridge_registry, BridgeRegistry);

	  if batches.is_empty() { return Err("Benchmark not found for this pallet.".into()) }
	  Ok(batches)
	}
  }
}
