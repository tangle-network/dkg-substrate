use std::marker::PhantomData;

use sc_client_api::{BlockchainEvents, HeaderBackend, AuxStore};
use sp_api::{BlockT, ProvideRuntimeApi, ApiExt, offchain::storage::InMemOffchainStorage};
use tokio::net::ToSocketAddrs;
use dkg_mock_blockchain::TestBlock;
use futures::{StreamExt, SinkExt};
use dkg_mock_blockchain::transport::ProtocolPacket;
use sp_api::StateBackend;
use sp_runtime::traits::BlakeTwo256;
use sc_client_api::UsageInfo;
use sp_runtime::offchain::storage::StorageValue;
use sc_client_api::StorageKey;

use crate::{worker::DKGWorker, gossip_engine::GossipEngineIface};

/// When peers use a Client, the streams they receive are suppose
/// to come from the BlockChain. However, for testing purposes, we will mock
/// the blockchain and thus send pseudo notifications to subscribing clients.
/// To allow flexibility in the design, each [`MockClient`] will need to connect
/// to a MockBlockchain service that periodically sends notifications to all connected
/// clients.
pub struct MockClient<GE> {
	_pd: PhantomData<GE>
}

pub struct TestBackend {

}

impl<GE: GossipEngineIface> MockClient<GE> {
	pub async fn connect<T: ToSocketAddrs>(mock_bc_addr: T, peer_id: Vec<u8>, dkg_worker: DKGWorker<TestBlock, TestBackend, Self, GE>) -> std::io::Result<Self> {
		let socket = tokio::net::TcpStream::connect(mock_bc_addr).await?;
		let task = async move {
			let (tx, mut rx) = dkg_mock_blockchain::transport::bind_transport::<TestBlock>(socket);

			while let Some(packet) = rx.next().await {
				match packet {
					ProtocolPacket::InitialHandshake => {
						// pong back the handshake response
						tx.send(ProtocolPacket::InitialHandshakeResponse { peer_id: peer_id.clone() }).await.unwrap();
					}
					ProtocolPacket::BlockChainToClient { event } => {
						
					}
					ProtocolPacket::Halt => {
						dkg_logging::info!(target: "dkg", "Received HALT command from the orchestrator");
						std::process::exit(0);
					}

					packet => {
						panic!("Received unexpected packet: {packet:?}")
					}
    			}
			}

			panic!("The connection to the MockBlockchain died")
		};
	}
}

impl<GE: GossipEngineIface> BlockchainEvents<TestBlock> for MockClient<GE> {
	fn finality_notification_stream(&self) -> sc_client_api::FinalityNotifications<TestBlock> {
		TracingUnboundedReceiver
	}

	fn import_notification_stream(&self) -> sc_client_api::ImportNotifications<TestBlock> {}

	fn storage_changes_notification_stream(
		&self,
		filter_keys: Option<&[sc_client_api::StorageKey]>,
		child_filter_keys: Option<
			&[(sc_client_api::StorageKey, Option<Vec<sc_client_api::StorageKey>>)],
		>,
	) -> sp_blockchain::Result<sc_client_api::StorageEventStream<<TestBlock as BlockT>::Hash>> {
	}
}


impl sc_client_api::Backend<TestBlock> for TestBackend {
    type BlockImportOperation = sc_client_api::in_mem::BlockImportOperation<TestBlock>;

    type Blockchain = sc_client_api::in_mem::Blockchain<TestBlock>;

    type State = DummyStateBackend;

    type OffchainStorage = InMemOffchainStorage;

    fn begin_operation(&self) -> sp_blockchain::Result<Self::BlockImportOperation> {
        todo!()
    }

    fn begin_state_operation(
		&self,
		operation: &mut Self::BlockImportOperation,
		block: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn commit_operation(
		&self,
		transaction: Self::BlockImportOperation,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn finalize_block(
		&self,
		hash: <TestBlock as BlockT>::Hash,
		justification: Option<sp_runtime::Justification>,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn append_justification(
		&self,
		hash: <TestBlock as BlockT>::Hash,
		justification: sp_runtime::Justification,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn blockchain(&self) -> &Self::Blockchain {
        todo!()
    }

    fn usage_info(&self) -> Option<sc_client_api::UsageInfo> {
        todo!()
    }

    fn offchain_storage(&self) -> Option<Self::OffchainStorage> {
        todo!()
    }

    fn state_at(&self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<Self::State> {
        todo!()
    }

    fn revert(
		&self,
		n: sp_api::NumberFor<TestBlock>,
		revert_finalized: bool,
	) -> sp_blockchain::Result<(sp_api::NumberFor<TestBlock>, std::collections::HashSet<<TestBlock as BlockT>::Hash>)> {
        todo!()
    }

    fn remove_leaf_block(&self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn get_import_lock(&self) -> &RwLock<()> {
        todo!()
    }

    fn requires_full_sync(&self) -> bool {
        todo!()
    }
}


impl<GE: GossipEngineIface> HeaderBackend<TestBlock> for MockClient<GE> {
    fn header(&self, id: sp_api::BlockId<TestBlock>) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Header>> {
        todo!()
    }

    fn info(&self) -> sp_blockchain::Info<TestBlock> {
        todo!()
    }

    fn status(&self, id: sp_api::BlockId<TestBlock>) -> sp_blockchain::Result<sp_blockchain::BlockStatus> {
        todo!()
    }

    fn number(
		&self,
		hash: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<Option<<<TestBlock as BlockT>::Header as sp_api::HeaderT>::Number>> {
        todo!()
    }

    fn hash(&self, number: sp_api::NumberFor<TestBlock>) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Hash>> {
        todo!()
    }
}


impl<GE: GossipEngineIface> ProvideRuntimeApi<TestBlock> for MockClient<GE> {
	type Api = DummyApi;
	fn runtime_api(&self) -> sp_api::ApiRef<Self::Api> {
		sp_api::ApiRef::from(DummyApi)
	}
}
impl AuxStore for TestBackend {
    fn insert_aux<
		'a,
		'b: 'a,
		'c: 'a,
		I: IntoIterator<Item = &'a (&'c [u8], &'c [u8])>,
		D: IntoIterator<Item = &'a &'b [u8]>,
	>(
		&self,
		insert: I,
		delete: D,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn get_aux(&self, key: &[u8]) -> sp_blockchain::Result<Option<Vec<u8>>> {
        todo!()
    }
}

struct DummyApi;
#[derive(Debug)]
struct DummyStateBackend;

impl StateBackend<BlakeTwo256> for DummyStateBackend {
    type Error;

    type Transaction;

    type TrieBackendStorage;

    fn storage(&self, key: &[u8]) -> Result<Option<StorageValue>, Self::Error> {
        todo!()
    }

    fn storage_hash(&self, key: &[u8]) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
        todo!()
    }

    fn child_storage(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<StorageValue>, Self::Error> {
        todo!()
    }

    fn child_storage_hash(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
        todo!()
    }

    fn next_storage_key(&self, key: &[u8]) -> Result<Option<StorageKey>, Self::Error> {
        todo!()
    }

    fn next_child_storage_key(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<StorageKey>, Self::Error> {
        todo!()
    }

    fn apply_to_key_values_while<F: FnMut(Vec<u8>, Vec<u8>) -> bool>(
		&self,
		child_info: Option<&sc_client_api::ChildInfo>,
		prefix: Option<&[u8]>,
		start_at: Option<&[u8]>,
		f: F,
		allow_missing: bool,
	) -> Result<bool, Self::Error> {
        todo!()
    }

    fn apply_to_keys_while<F: FnMut(&[u8]) -> bool>(
		&self,
		child_info: Option<&sc_client_api::ChildInfo>,
		prefix: Option<&[u8]>,
		start_at: Option<&[u8]>,
		f: F,
	) {
        todo!()
    }

    fn for_key_values_with_prefix<F: FnMut(&[u8], &[u8])>(&self, prefix: &[u8], f: F) {
        todo!()
    }

    fn for_child_keys_with_prefix<F: FnMut(&[u8])>(
		&self,
		child_info: &sc_client_api::ChildInfo,
		prefix: &[u8],
		f: F,
	) {
        todo!()
    }

    fn storage_root<'a>(
		&self,
		delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, Self::Transaction)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord {
        todo!()
    }

    fn child_storage_root<'a>(
		&self,
		child_info: &sc_client_api::ChildInfo,
		delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, bool, Self::Transaction)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord {
        todo!()
    }

    fn pairs(&self) -> Vec<(StorageKey, StorageValue)> {
        todo!()
    }

    fn register_overlay_stats(&self, _stats: &crate::stats::StateMachineStats) {
        todo!()
    }

    fn usage_info(&self) -> UsageInfo {
        todo!()
    }
}

impl ApiExt<TestBlock> for DummyApi {
    type StateBackend = DummyStateBackend;

    fn execute_in_transaction<F: FnOnce(&Self) -> sp_api::TransactionOutcome<R>, R>(&self, call: F) -> R
	where
		Self: Sized {
        todo!()
    }

    fn has_api<A: sp_api::RuntimeApiInfo + ?Sized>(&self, at: &sp_api::BlockId<TestBlock>) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn has_api_with<A: sp_api::RuntimeApiInfo + ?Sized, P: Fn(u32) -> bool>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
		pred: P,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn api_version<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
	) -> Result<Option<u32>, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn record_proof(&mut self) {
        todo!()
    }

    fn extract_proof(&mut self) -> Option<sc_client_api::StorageProof> {
        todo!()
    }

    fn proof_recorder(&self) -> Option<sp_api::ProofRecorder<TestBlock>> {
        todo!()
    }

    fn into_storage_changes(
		&self,
		backend: &Self::StateBackend,
		parent_hash: <TestBlock as BlockT>::Hash,
	) -> Result<sp_api::StorageChanges<Self::StateBackend, TestBlock>, String>
	where
		Self: Sized {
        todo!()
    }
}