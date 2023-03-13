use crate::{
	mock_blockchain_config::MockBlockchainConfig, transport::*, MockBlockChainEvent, TestCase, BlockTraitForTest, AttachedCommandMetadata, FinalityNotification,
};
use atomic::Atomic;
use futures::{SinkExt, StreamExt};
use std::{
	collections::{HashMap, VecDeque},
	net::SocketAddr,
	sync::{atomic::Ordering, Arc},
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::{mpsc, Mutex, RwLock},
};
use uuid::Uuid;

// TODO: replace with peer id
type PeerId = Vec<u8>;

#[derive(Clone)]
pub struct MockBlockchain<B: crate::BlockTraitForTest> {
	listener: Arc<Mutex<Option<TcpListener>>>,
	config: MockBlockchainConfig,
	clients: Arc<RwLock<HashMap<PeerId, ConnectedClientState<B>>>>,
	// client sub-tasks communicate with the orchestrator using this sender
	to_orchestrator: mpsc::UnboundedSender<ClientToOrchestratorEvent>,
	// the orchestrator receives updates from its client sub-tasks from this receiver
	orchestrator_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ClientToOrchestratorEvent>>>>,
	orchestrator_state: Arc<Atomic<OrchestratorState>>,
}

/// For communicating between the orchestrator task and each spawned client sub-task
#[derive(Debug)]
enum ClientToOrchestratorEvent {
	// Once the client has completed its handshake with the mock blockchain,
	// the client sends this to the orchestrator. The orchestrator will begin
	// running the test cases once "n" peers send this status
	ClientReady,
	TestResult { peer_id: PeerId, trace_id: Uuid, result: TestResult },
}

#[derive(Debug)]
struct TestResult {
	success: bool,
	error_message: Option<String>,
}

#[derive(Debug)]
enum OrchestratorToClientEvent<B: crate::BlockTraitForTest> {
	// Tells the client subtask to halt
	Halt,
	// Tells the client subtask to begin a test
	BeginTest { trace_id: Uuid, test: TestCase },
	// Tells the client subtask to send a mock event
	BlockChainEvent(MockBlockChainEvent<B>),
}

#[derive(Copy, Clone, Default, Debug)]
enum OrchestratorState {
	#[default]
	// The orchestrator is waiting for n clients to connect
	WaitingForInit,
	// The orchestrator dispatched a round, and, is waiting for the clients
	// to submit a status update back
	AwaitingRoundCompletion,
	// All test cases have been driven to completion
	Complete,
}

struct ConnectedClientState<B: crate::BlockTraitForTest> {
	// a map from tracing id => test case. Once the test case passes
	// for the specific client, the test case will be removed from the list
	outstanding_tasks: HashMap<Uuid, crate::TestCase>,
	orchestrator_to_client_subtask: mpsc::UnboundedSender<OrchestratorToClientEvent<B>>,
}

impl<B: crate::BlockTraitForTest> MockBlockchain<B> {
	pub async fn new(config: MockBlockchainConfig) -> std::io::Result<Self> {
		let listener = TcpListener::bind(&config.bind).await?;
		let clients = Arc::new(RwLock::new(HashMap::new()));
		let (to_orchestrator, orchestrator_rx) = mpsc::unbounded_channel();
		let orchestrator_state = Arc::new(Atomic::new(Default::default()));

		Ok(Self {
			listener: Arc::new(Mutex::new(Some(listener))),
			config,
			clients,
			orchestrator_state,
			to_orchestrator,
			orchestrator_rx: Arc::new(Mutex::new(Some(orchestrator_rx))),
		})
	}

	pub async fn execute(self) -> std::io::Result<()> {
		let listener = self.listener.lock().await.take().unwrap();
		let this_orchestrator = self.clone();

		// the listener task takes client streams and handles them
		let listener_task = async move {
			while let Ok((stream, addr)) = listener.accept().await {
				let this = self.clone();
				// spawn a sub-task within this listener task
				let _ = tokio::task::spawn(this.handle_stream(stream, addr));
			}

			Err::<(), _>(generic_error("Listener died"))
		};

		// the orchestrator task is what sends events to each spawned sub-task within the listener
		// task
		let orchestrator_task = this_orchestrator.orchestrate();

		tokio::try_join!(listener_task, orchestrator_task).map(|_| ())
	}

	/// For debugging purposes, everything will get unwrapped here
	async fn handle_stream(self, stream: TcpStream, addr: SocketAddr) {
		let (mut tx, mut rx) = bind_transport::<B>(stream);
		// begin handshake process
		let handshake_packet = ProtocolPacket::InitialHandshake;
		tx.send(handshake_packet).await.unwrap();
		let response = rx.next().await.unwrap();

		if let ProtocolPacket::InitialHandshakeResponse { peer_id } = response {
			log::info!(target: "dkg", "Received handshake response from peer {peer_id:?} = {addr:?}");
			let mut write = self.clients.write().await;

			// create a channel for allowing the orchestrator to send this sub-task commands
			let (orchestrator_to_this_task, mut orchestrator_rx) = mpsc::unbounded_channel();
			let state = ConnectedClientState {
				outstanding_tasks: Default::default(),
				orchestrator_to_client_subtask: orchestrator_to_this_task,
			};

			if write.insert(peer_id.clone(), state).is_some() {
				// when simulating disconnects, this may happen
				log::warn!(target: "dkg", "Inserted peer {peer_id:?} into the clients map, but, overwrote a previous value")
			}

			std::mem::drop(write);

			// Tell the orchestrator we have established a connection with the client
			self.to_orchestrator
				.send(ClientToOrchestratorEvent::ClientReady)
				.unwrap();

			let peer_id = &peer_id;

			// this subtask handles passing messages from the DKG client to the orchestrator
			// for tallying results
			let fwd_orchestrator = async move {
				while let Some(packet) = rx.next().await {
					match packet {
						pkt @ ProtocolPacket::InitialHandshake |
						pkt @ ProtocolPacket::InitialHandshakeResponse { .. } |
						pkt @ ProtocolPacket::BlockChainToClient { .. } |
						pkt @ ProtocolPacket::Halt => {
							panic!("Received invalid packet {pkt:?} inside to_orchestrator for {peer_id:?}")
						}
						ProtocolPacket::ClientToBlockChain { event } => {
							let trace_id = event.trace_id;
							let result = TestResult {
								error_message: event.error,
								success: event.success,
							};
							self.to_orchestrator.send(ClientToOrchestratorEvent::TestResult { peer_id: peer_id.clone(), trace_id, result }).unwrap();
						}
					}
				}
			};

			// this subtask handles receiving commands from the orhcestrator and potentially
			// sending testcases to the DKG clients
			let from_orchestrator = async move {
				while let Some(orchestrator_command) = orchestrator_rx.recv().await {
					match orchestrator_command {
						OrchestratorToClientEvent::Halt => {
							log::info!(target: "dkg", "Peer {peer_id:?} has been requested to halt");
							// tell the subscribing client to shutdown the DKG
							tx.send(ProtocolPacket::Halt).await.unwrap();
							return
						},
						OrchestratorToClientEvent::BeginTest { trace_id, test } => {
							tx.send(ProtocolPacket::BlockChainToClient {
								event: MockBlockChainEvent::TestCase { trace_id, test },
							})
							.await
							.unwrap();
						},
						OrchestratorToClientEvent::BlockChainEvent(event) => {
							tx.send(ProtocolPacket::BlockChainToClient { event }).await.unwrap();
						},
					}
				}
			};

			let _ = tokio::join!(fwd_orchestrator, from_orchestrator);

			panic!("Communications between orchestrator and DKG client for peer {peer_id:?} died")
		} else {
			panic!("Invalid first packet received from peer")
		}
	}

	async fn orchestrate(self) -> std::io::Result<()> {
		let mut test_cases = self.generate_test_cases();
		let mut client_to_orchestrator_rx = self.orchestrator_rx.lock().await.take().unwrap();
		let mut current_round_completed_count = 0;

		while let Some(client_update) = client_to_orchestrator_rx.recv().await {
			match self.orchestrator_state.load(Ordering::SeqCst) {
				o_state @ OrchestratorState::WaitingForInit => match client_update {
					ClientToOrchestratorEvent::ClientReady => {
						let clients = self.clients.read().await;
						// NOTE: the client automatically puts its handle inside this map. We do not have to here
						if clients.len() == self.config.min_clients {
							// we are ready to begin testing rounds
							std::mem::drop(clients);
							self.orchestrator_begin_next_round(&mut test_cases).await;
						}
					},

					c_update => log_invalid_signal(&o_state, &c_update),
				},

				o_state @ OrchestratorState::AwaitingRoundCompletion => {
					match &client_update {
						ClientToOrchestratorEvent::ClientReady =>
							log_invalid_signal(&o_state, &client_update),
						ClientToOrchestratorEvent::TestResult { peer_id, trace_id, result } => {
							let mut clients = self.clients.write().await;
							let client = clients.get_mut(peer_id).unwrap();
							if result.success {
								log::info!(target: "dkg", "Peer {peer_id:?} successfully completed test {trace_id:?}");
								// remove from map
								assert!(client.outstanding_tasks.remove(trace_id).is_some());
							} else {
								log::error!(target: "dkg", "Peer {peer_id:?} unsuccessfully completed test {trace_id:?}. Reason: {:?}", result.error_message);
								// do not remove from map. At the end , any remaining tasks will
								// cause the orchestrator to have a nonzero exit code (useful for
								// pipeline testing)
							}

							// regardless of success, increment completed count for the current
							// round
							current_round_completed_count += 1;
						},
					}

					// at the end, check if the round is complete
					if current_round_completed_count == self.config.min_clients {
						current_round_completed_count = 0; // reset to 0 for next round
						self.orchestrator_begin_next_round(&mut test_cases).await
					}
				},
				o_state @ OrchestratorState::Complete =>
					log_invalid_signal(&o_state, &client_update),
			}
		}

		Err(generic_error("client_to_orchestrator_tx's all dropped"))
	}

	fn generate_test_cases(&self) -> VecDeque<TestCase> {
		let error_cases = &self.config.error_cases;
		let mut test_cases = VecDeque::new();

		// add all positive cases to the front
		for _ in 0..self.config.positive_cases {
			test_cases.push_back(TestCase::Valid)
		}

		// add all error cases to the back
		for error_case in error_cases {
			for _ in 0..error_case.count {
				test_cases.push_back(TestCase::Invalid(error_case.clone()))
			}
		}

		test_cases
	}

	async fn orchestrator_begin_next_round(&self, test_cases: &mut VecDeque<TestCase>) {
		if let Some(next_case) = test_cases.pop_front() {
			self.orchestrator_set_state(OrchestratorState::AwaitingRoundCompletion);
			let mut write = self.clients.write().await;
			for (_id, client) in write.iter_mut() {
				let trace_id = Uuid::new_v4();
				client.outstanding_tasks.insert(trace_id, next_case.clone());

				// First, send out a MockBlockChainEvent
				todo!();

				client
					.orchestrator_to_client_subtask
					.send(OrchestratorToClientEvent::BeginTest {
						trace_id,
						test: next_case.clone(),
					})
					.unwrap();
			}
		} else {
			log::info!(target: "dkg", "Orchestrator has finished running all tests");
			self.orchestrator_set_state(OrchestratorState::Complete);
			let mut exit_code = 0;
			// check to see the final state
			let read = self.clients.read().await;
			for (peer_id, client_state) in &*read {
				let outstanding_tasks = &client_state.outstanding_tasks;
				// the client should have no outstanding tasks if successful
				let success = outstanding_tasks.is_empty();
				if !success {
					exit_code = 1;
					log::info!(target: "dkg", "Peer {peer_id:?} final state FAILURE | Failed tasks: {outstanding_tasks:?}")
				} else {
					log::info!(target: "dkg", "Peer {peer_id:?} SUCCESS!")
				}
				client_state
					.orchestrator_to_client_subtask
					.send(OrchestratorToClientEvent::Halt)
					.unwrap();
			}

			// Give time for the client subtasks to send relevent packets to the DKG clients
			tokio::time::sleep(std::time::Duration::from_millis(500)).await;
			std::process::exit(exit_code);
		}
	}

	fn orchestrator_set_state(&self, state: OrchestratorState) {
		self.orchestrator_state.store(state, Ordering::SeqCst);
	}
}

fn generic_error<T: Into<String>>(err: T) -> std::io::Error {
	std::io::Error::new(std::io::ErrorKind::Other, err.into())
}

fn log_invalid_signal(o_state: &OrchestratorState, c_update: &ClientToOrchestratorEvent) {
	log::error!(target: "dkg", "Orchestrator state is {o_state:?}, yet, the client's update state is {c_update:?}")
}

fn create_mocked_finality_blockchain_event<B: BlockTraitForTest>() -> MockBlockChainEvent<B> {
	let notification = FinalityNotification {
		hash: Default::default(),
		header: Default::default(),
		tree_route: Arc::new([]),
		stale_heads: Arc::new([]),
	};
	MockBlockChainEvent::FinalityNotification { notification }
}