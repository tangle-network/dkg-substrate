use dkg_gadget::debug_logger::DebugLogger;
use dkg_mock_blockchain::{
	transport::ProtocolPacket, FinalityNotification, ImportNotification, MockBlockchainEvent,
	MockClientResponse, TestBlock,
};

use futures::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use sc_client_api::{AuxStore, BlockchainEvents, HeaderBackend};
use sc_network::PeerId;
use sc_utils::mpsc::*;
use sp_api::{offchain::storage::InMemOffchainStorage, BlockT, ProvideRuntimeApi};

use crate::dummy_api::*;
use dkg_gadget::worker::TestClientPayload;
use sp_runtime::testing::H256;
use std::{collections::HashMap, sync::Arc};
use tokio::{net::ToSocketAddrs, sync::mpsc::UnboundedReceiver};
use uuid::Uuid;

/// When peers use a Client, the streams they receive are suppose
/// to come from the BlockChain. However, for testing purposes, we will mock
/// the blockchain and thus send pseudo notifications to subscribing clients.
/// To allow flexibility in the design, each [`MockClient`] will need to connect
/// to a MockBlockchain service that periodically sends notifications to all connected
/// clients.
#[derive(Clone)]
pub struct TestClient {
	inner: TestClientState,
}

#[derive(Clone)]
pub struct TestClientState {
	finality_stream: Arc<MultiSubscribableStream<FinalityNotification<TestBlock>>>,
	import_stream: Arc<MultiSubscribableStream<ImportNotification<TestBlock>>>,
	api: DummyApi,
	offchain_storage: InMemOffchainStorage,
	local_test_cases: LocalTestCases,
}

pub type LocalTestCases = Arc<Mutex<HashMap<Uuid, Option<Result<(), String>>>>>;

impl TestClient {
	/// Connects to the test blockchain and starts listening for events
	/// from the blockchain. The test blockchain will send events to the client
	/// and this client will send updates to the blockchain.
	pub async fn connect<T: ToSocketAddrs>(
		mock_bc_addr: T,
		peer_id: PeerId,
		api: DummyApi,
		mut from_dkg_worker: UnboundedReceiver<TestClientPayload>,
		latest_test_uuid: Arc<RwLock<Option<Uuid>>>,
		logger: DebugLogger,
	) -> std::io::Result<Self> {
		logger
			.info(format!("0. Setting up orchestrator<=>DKG communications for peer {peer_id:?}"));
		let socket = tokio::net::TcpStream::connect(mock_bc_addr).await?;
		let (tx, mut rx) = dkg_mock_blockchain::transport::bind_transport::<TestBlock>(socket);
		let tx0 = Arc::new(tokio::sync::Mutex::new(tx));
		let tx1 = tx0.clone();

		let this = TestClient {
			inner: TestClientState {
				finality_stream: Arc::new(MultiSubscribableStream::new(
					"finality_stream",
					logger.clone(),
				)),
				import_stream: Arc::new(MultiSubscribableStream::new(
					"import_stream",
					logger.clone(),
				)),
				api,
				offchain_storage: Default::default(),
				local_test_cases: Arc::new(Mutex::new(Default::default())),
			},
		};

		let _this_for_dkg_listener = this.clone();
		let logger0 = logger.clone();
		let dkg_worker_listener = async move {
			while let Some((trace_id, result, pub_key)) = from_dkg_worker.recv().await {
				logger0.info(format!(
					"The client {peer_id:?} has finished test {trace_id:?}. Result: {result:?}"
				));

				let event = if let Some(pub_key) = pub_key {
					MockClientResponse::Keygen { result, trace_id, pub_key }
				} else {
					MockClientResponse::Sign { result, trace_id }
				};

				let packet = ProtocolPacket::ClientToBlockchain { event };
				tx0.lock().await.send(packet).await.unwrap();
			}

			panic!("DKG worker listener ended prematurely")
		};

		let this_for_orchestrator_rx = this.clone();
		let orchestrator_coms = async move {
			logger
				.info(format!("Complete: orchestrator<=>DKG communications for peer {peer_id:?}"));
			while let Some(packet) = rx.next().await {
				logger.info("Received a packet");
				match packet {
					ProtocolPacket::InitialHandshake => {
						// pong back the handshake response
						tx1.lock()
							.await
							.send(ProtocolPacket::InitialHandshakeResponse { peer_id })
							.await
							.unwrap();
					},
					ProtocolPacket::BlockchainToClient { trace_id, event } => {
						*latest_test_uuid.write() = Some(trace_id);

						match event {
							MockBlockchainEvent::FinalityNotification { notification } => {
								this_for_orchestrator_rx
									.inner
									.local_test_cases
									.lock()
									.insert(trace_id, None);
								logger.info("Passing finality notification to the client");
								this_for_orchestrator_rx.inner.finality_stream.send(notification);
							},
							MockBlockchainEvent::ImportNotification { notification } => {
								this_for_orchestrator_rx
									.inner
									.local_test_cases
									.lock()
									.insert(trace_id, None);
								logger.info("Passing import notification to the client");
								this_for_orchestrator_rx.inner.import_stream.send(notification);
							},
						}
					},
					ProtocolPacket::Halt => {
						logger.info("Received HALT command from the orchestrator".to_string());
						return
					},

					packet => {
						panic!("Received unexpected packet: {packet:?}")
					},
				}
			}

			panic!("The connection to the MockBlockchain died")
		};

		tokio::task::spawn(dkg_worker_listener);
		tokio::task::spawn(orchestrator_coms);

		Ok(this)
	}
}

// Each stream may be called multiple times. As such, we need to keep track of each subscriber
// and broadcast as necessary. This is what the MultiSubscribableStream does. It allows subscribers
// to arbitrarily subscribe to the stream and receive all events.
pub struct MultiSubscribableStream<T> {
	inner: parking_lot::RwLock<Vec<TracingUnboundedSender<T>>>,
	logger: DebugLogger,
	tag: &'static str,
}

impl<T: Clone> MultiSubscribableStream<T> {
	pub fn new(tag: &'static str, logger: DebugLogger) -> Self {
		Self { inner: parking_lot::RwLock::new(vec![]), tag, logger }
	}

	pub fn subscribe(&self) -> TracingUnboundedReceiver<T> {
		let (tx, rx) = tracing_unbounded(self.tag, 999999);
		let mut lock = self.inner.write();
		lock.push(tx);
		rx
	}

	pub fn send(&self, t: T) {
		let mut lock = self.inner.write();

		if lock.is_empty() {
			self.logger.error("No subscribers to send to");
		}

		assert!(!lock.is_empty());
		let count_init = lock.len();
		// receiver will naturally drop when no longer used.
		lock.retain(|tx| tx.unbounded_send(t.clone()).is_ok());
		let diff = count_init - lock.len();
		self.logger.info(format!("Dropped {diff} subscribers (init: {count_init}"));
	}
}

impl BlockchainEvents<TestBlock> for TestClient {
	fn finality_notification_stream(&self) -> sc_client_api::FinalityNotifications<TestBlock> {
		self.inner.finality_stream.subscribe()
	}

	fn import_notification_stream(&self) -> sc_client_api::ImportNotifications<TestBlock> {
		self.inner.import_stream.subscribe()
	}

	fn storage_changes_notification_stream(
		&self,
		_filter_keys: Option<&[sc_client_api::StorageKey]>,
		_child_filter_keys: Option<
			&[(sc_client_api::StorageKey, Option<Vec<sc_client_api::StorageKey>>)],
		>,
	) -> sp_blockchain::Result<sc_client_api::StorageEventStream<<TestBlock as BlockT>::Hash>> {
		todo!()
	}

	fn every_import_notification_stream(&self) -> sc_client_api::ImportNotifications<TestBlock> {
		todo!()
	}
}

impl sc_client_api::Backend<TestBlock> for TestClient {
	type BlockImportOperation = DummyStateBackend;
	type Blockchain = sc_client_api::in_mem::Blockchain<TestBlock>;
	type State = DummyStateBackend;
	type OffchainStorage = InMemOffchainStorage;

	fn pin_block(
		&self,
		_: <TestBlock as sp_api::BlockT>::Hash,
	) -> Result<(), sp_blockchain::Error> {
		todo!()
	}
	fn unpin_block(&self, _: <TestBlock as sp_api::BlockT>::Hash) {
		todo!()
	}

	fn begin_operation(&self) -> sp_blockchain::Result<Self::BlockImportOperation> {
		todo!()
	}

	fn begin_state_operation(
		&self,
		_operation: &mut Self::BlockImportOperation,
		_block: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn commit_operation(
		&self,
		_transaction: Self::BlockImportOperation,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn finalize_block(
		&self,
		_hash: <TestBlock as BlockT>::Hash,
		_justification: Option<sp_runtime::Justification>,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn append_justification(
		&self,
		_hash: <TestBlock as BlockT>::Hash,
		_justification: sp_runtime::Justification,
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
		Some(self.inner.offchain_storage.clone())
	}

	fn state_at(&self, _hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<Self::State> {
		todo!()
	}

	fn revert(
		&self,
		_n: sp_api::NumberFor<TestBlock>,
		_revert_finalized: bool,
	) -> sp_blockchain::Result<(
		sp_api::NumberFor<TestBlock>,
		std::collections::HashSet<<TestBlock as BlockT>::Hash>,
	)> {
		todo!()
	}

	fn remove_leaf_block(&self, _hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn get_import_lock(&self) -> &RwLock<()> {
		todo!()
	}

	fn requires_full_sync(&self) -> bool {
		todo!()
	}
}

impl HeaderBackend<TestBlock> for TestClient {
	fn header(&self, _id: H256) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Header>> {
		todo!()
	}

	fn info(&self) -> sp_blockchain::Info<TestBlock> {
		todo!()
	}

	fn status(&self, _id: H256) -> sp_blockchain::Result<sp_blockchain::BlockStatus> {
		todo!()
	}

	fn number(
		&self,
		_hash: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<Option<<<TestBlock as BlockT>::Header as sp_api::HeaderT>::Number>> {
		todo!()
	}

	fn hash(
		&self,
		_number: sp_api::NumberFor<TestBlock>,
	) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Hash>> {
		todo!()
	}
}

impl ProvideRuntimeApi<TestBlock> for TestClient {
	type Api = DummyApi;
	fn runtime_api(&self) -> sp_api::ApiRef<Self::Api> {
		sp_api::ApiRef::from(self.inner.api.clone())
	}
}
impl AuxStore for TestClient {
	fn insert_aux<
		'a,
		'b: 'a,
		'c: 'a,
		I: IntoIterator<Item = &'a (&'c [u8], &'c [u8])>,
		D: IntoIterator<Item = &'a &'b [u8]>,
	>(
		&self,
		_insert: I,
		_delete: D,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn get_aux(&self, _key: &[u8]) -> sp_blockchain::Result<Option<Vec<u8>>> {
		todo!()
	}
}
