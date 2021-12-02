use crate as pallet_dkg_proposal_handler;
use codec::Encode;
use frame_support::{parameter_types, traits::Everything, PalletId};
use frame_system as system;
use sp_core::{ecdsa::Signature, H256};
use sp_runtime::{
	testing::{Header, TestXt},
	traits::{BlakeTwo256, Extrinsic as ExtrinsicT, IdentifyAccount, IdentityLookup, Verify},
};

use sp_core::offchain::{testing, OffchainDbExt, OffchainWorkerExt, TransactionPoolExt};

use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};

use sp_runtime::RuntimeAppPublic;

use dkg_runtime_primitives::{keccak_256, TransactionV2};

use frame_support::traits::{OnFinalize, OnInitialize};

use dkg_runtime_primitives::{EIP2930Transaction, ProposalType, TransactionAction, U256};
use std::sync::Arc;

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
		DKGProposals: pallet_dkg_proposals::{Pallet, Call, Storage, Event<T>},
		DKGProposalHandler: pallet_dkg_proposal_handler::{Pallet, Call, Storage, Event<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

parameter_types! {
	pub const ChainIdentifier: u32 = 5;
	pub const ProposalLifetime: u64 = 50;
	pub const DKGAccountId: PalletId = PalletId(*b"dw/dkgac");
	pub const GracePeriod: u64 = 10;
}

type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl system::Config for Test {
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type Origin = Origin;
	type Call = Call;
	type Index = u64;
	type BlockNumber = u64;
	type Hash = H256;
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

impl pallet_dkg_proposal_handler::Config for Test {
	type Event = Event;
	type ChainId = u32;
	type OffChainAuthId = dkg_runtime_primitives::crypto::OffchainAuthId;
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
}

const PHRASE: &str = "news slush supreme milk chapter athlete soap sausage put clutch what kitten";

#[allow(dead_code)]
pub fn new_test_ext() -> sp_io::TestExternalities {
	system::GenesisConfig::default().build_storage::<Test>().unwrap().into()
}

pub fn execute_test_with<R>(execute: impl FnOnce() -> R) -> R {
	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let keystore = KeyStore::new();
	let (pool, _pool_state) = testing::TestTransactionPoolExt::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(KeystoreExt(Arc::new(keystore)));
	t.register_extension(TransactionPoolExt::new(pool));

	t.execute_with(execute)
}

pub fn run_to_block(n: u64) {
	while System::block_number() < n {
		System::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
	}
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
	let (pool, _pool_state) = testing::TestTransactionPoolExt::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let pub_key =
		SyncCryptoStore::ecdsa_public_keys(&keystore, dkg_runtime_primitives::crypto::Public::ID)
			.get(0)
			.unwrap()
			.clone();

	keystore.ecdsa_sign_prehashed(dkg_runtime_primitives::crypto::Public::ID, &pub_key, msg)
}

pub fn mock_signed_proposal(eth_tx: TransactionV2) -> ProposalType {
	let eth_tx_ser = eth_tx.encode();

	let hash = keccak_256(&eth_tx_ser);
	let sig = mock_sign_msg(&hash).unwrap().unwrap();

	let mut sig_vec: Vec<u8> = Vec::new();
	sig_vec.extend_from_slice(&sig.0);

	return ProposalType::EVMSigned { data: eth_tx_ser.clone(), signature: sig_vec }
}
