use std::sync::Arc;

use crate::{mock::*, Error};
use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	LegacyTransaction, OffchainSignedProposals, ProposalType, TransactionAction,
	TransactionSignature, TransactionV2, OFFCHAIN_SIGNED_PROPOSALS, U256,
};
use frame_support::assert_ok;
use sp_core::H256;
use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};
use sp_runtime::{
	offchain::{
		storage::StorageValueRef, testing, OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
	},
	RuntimeAppPublic,
};

const LOWER: H256 = H256([
	0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
]);
const UPPER: H256 = H256([
	0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
	0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x40,
]);

fn construct_evm_tx(nonce_val: u64) -> TransactionV2 {
	TransactionV2::Legacy(LegacyTransaction {
		nonce: U256([nonce_val; 4]),
		gas_price: Default::default(),
		gas_limit: Default::default(),
		action: TransactionAction::Create,
		value: Default::default(),
		input: Default::default(),
		signature: TransactionSignature::new(27, LOWER, UPPER).unwrap(),
	})
}

#[test]
fn should_submit_signed_proposal_that_is_not_onchain() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";

	let tx = construct_evm_tx(0);

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		let props_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut offchain_proposals = OffchainSignedProposals::default();

		offchain_proposals.proposals.push_back(ProposalType::EVMSigned {
			data: tx.encode(),
			signature: Default::default(),
		});

		props_ref.set(&offchain_proposals.encode());

		let encoded_data = props_ref.get::<Vec<u8>>().unwrap().unwrap();

		let offchain_proposals = OffchainSignedProposals::decode(&mut &encoded_data[..]).unwrap();

		assert!(!offchain_proposals.proposals.is_empty());

		assert_ok!(DKGProposalHandler::submit_signed_proposal_onchain(0));

		let tr = pool_state.write().transactions.pop().unwrap();

		assert!(pool_state.read().transactions.is_empty());
		let tr = Extrinsic::decode(&mut &*tr).unwrap();
		assert_eq!(tr.signature.unwrap().0, 0);
		assert_eq!(
			tr.call,
			Call::DKGProposalHandler(crate::Call::submit_signed_proposal {
				prop: ProposalType::EVMSigned { data: tx.encode(), signature: Default::default() }
			})
		);

		let encoded_data = props_ref.get::<Vec<u8>>().unwrap().unwrap();

		let offchain_proposals = OffchainSignedProposals::decode(&mut &encoded_data[..]).unwrap();

		assert!(offchain_proposals.proposals.is_empty())
	});
}

#[test]
fn should_not_submit_signed_proposal_existing_on_chain() {
	const PHRASE: &str =
		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
	let nonce = U256([0; 4]).as_u64();
	let tx = construct_evm_tx(0);
	let chain_id = 0u32;

	let (offchain, _offchain_state) = testing::TestOffchainExt::new();
	let (pool, pool_state) = testing::TestTransactionPoolExt::new();
	let keystore = KeyStore::new();

	SyncCryptoStore::ecdsa_generate_new(
		&keystore,
		dkg_runtime_primitives::crypto::Public::ID,
		Some(PHRASE),
	)
	.unwrap();

	let mut t = sp_io::TestExternalities::default();
	t.register_extension(OffchainDbExt::new(offchain.clone()));
	t.register_extension(OffchainWorkerExt::new(offchain));
	t.register_extension(TransactionPoolExt::new(pool));
	t.register_extension(KeystoreExt(Arc::new(keystore)));

	t.execute_with(|| {
		let props_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut offchain_proposals = OffchainSignedProposals::default();

		offchain_proposals.proposals.push_back(ProposalType::EVMSigned {
			data: tx.encode(),
			signature: Default::default(),
		});

		props_ref.set(&offchain_proposals.encode());

		let encoded_data = props_ref.get::<Vec<u8>>().unwrap().unwrap();

		let offchain_proposals = OffchainSignedProposals::decode(&mut &encoded_data[..]).unwrap();

		assert!(!offchain_proposals.proposals.is_empty());

		crate::pallet::SignedProposals::<Test>::insert(
			chain_id,
			nonce,
			ProposalType::EVMSigned { data: tx.encode(), signature: Default::default() },
		);

		assert_ok!(DKGProposalHandler::submit_signed_proposal_onchain(0));

		assert!(pool_state.read().transactions.is_empty());

		let encoded_data = props_ref.get::<Vec<u8>>().unwrap().unwrap();

		let offchain_proposals = OffchainSignedProposals::decode(&mut &encoded_data[..]).unwrap();

		assert!(offchain_proposals.proposals.is_empty())
	});
}
