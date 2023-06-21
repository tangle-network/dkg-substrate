use crate::{
	mock_blockchain_config::MockBlockchainConfig, transport::*, FinalityNotification,
	MockBlockchainEvent, MockClientResponse, TestBlock, TestCase,
};
use atomic::Atomic;
use dkg_logging::debug_logger::DebugLogger;
use dkg_runtime_primitives::UnsignedProposal;
use futures::{SinkExt, StreamExt};
use sc_client_api::FinalizeSummary;
use sp_runtime::app_crypto::sp_core::hashing::sha2_256;
use std::{
	collections::{HashMap, VecDeque},
	net::SocketAddr,
	sync::{atomic::Ordering, Arc},
	time::Duration,
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::{mpsc, Mutex, RwLock},
};
use uuid::Uuid;

pub type PeerId = sc_network::PeerId;

#[derive(Clone)]
pub struct MockBlockchain<T: Clone> {
	listener: Arc<Mutex<Option<TcpListener>>>,
	config: MockBlockchainConfig,
	clients: Arc<RwLock<HashMap<PeerId, ConnectedClientState>>>,
	// client sub-tasks communicate with the orchestrator using this sender
	to_orchestrator: mpsc::UnboundedSender<ClientToOrchestratorEvent>,
	// the orchestrator receives updates from its client sub-tasks from this receiver
	orchestrator_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ClientToOrchestratorEvent>>>>,
	orchestrator_state: Arc<Atomic<OrchestratorState>>,
	blockchain: T,
	logger: DebugLogger,
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
enum TestResult {
	Keygen { result: Result<(), String>, pub_key: Vec<u8> },
	Sign { result: Result<(), String> },
}

#[derive(Debug)]
enum OrchestratorToClientEvent {
	// Tells the client subtask to halt
	Halt,
	// Tells the client subtask to send a mock event
	BlockChainEvent { trace_id: Uuid, event: MockBlockchainEvent<TestBlock> },
}

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
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

struct ConnectedClientState {
	// a map from tracing id => test case. Once the test case passes
	// for the specific client, the test case will be removed from the list
	outstanding_tasks_keygen: HashMap<Uuid, crate::TestCase>,
	outstanding_tasks_signing: HashMap<Uuid, Vec<crate::TestCase>>,
	orchestrator_to_client_subtask: mpsc::UnboundedSender<OrchestratorToClientEvent>,
}

impl<T: MutableBlockchain> MockBlockchain<T> {
	pub async fn new(
		config: MockBlockchainConfig,
		blockchain: T,
		logger: DebugLogger,
	) -> std::io::Result<Self> {
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
			blockchain,
			logger,
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
				tokio::task::spawn(this.handle_stream(stream, addr));
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
		let (mut tx, mut rx) = bind_transport::<TestBlock>(stream);
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
				outstanding_tasks_keygen: Default::default(),
				outstanding_tasks_signing: Default::default(),
				orchestrator_to_client_subtask: orchestrator_to_this_task,
			};

			if write.insert(peer_id, state).is_some() {
				// when simulating disconnects, this may happen
				log::warn!(target: "dkg", "Inserted peer {peer_id:?} into the clients map, but, overwrote a previous value")
			}

			std::mem::drop(write);

			// Tell the orchestrator we have established a connection with the client
			self.to_orchestrator.send(ClientToOrchestratorEvent::ClientReady).unwrap();

			let peer_id = &peer_id;

			// this subtask handles passing messages from the DKG client to the orchestrator
			// for tallying results
			let fwd_orchestrator = async move {
				while let Some(packet) = rx.next().await {
					match packet {
						pkt @ ProtocolPacket::InitialHandshake |
						pkt @ ProtocolPacket::InitialHandshakeResponse { .. } |
						pkt @ ProtocolPacket::BlockchainToClient { .. } |
						pkt @ ProtocolPacket::Halt => {
							panic!("Received invalid packet {pkt:?} inside to_orchestrator for {peer_id:?}")
						},
						ProtocolPacket::ClientToBlockchain { event } => {
							let (result, trace_id) = match event {
								MockClientResponse::Keygen { result, trace_id, pub_key } =>
									(TestResult::Keygen { result, pub_key }, trace_id),
								MockClientResponse::Sign { result, trace_id } =>
									(TestResult::Sign { result }, trace_id),
							};

							self.to_orchestrator
								.send(ClientToOrchestratorEvent::TestResult {
									peer_id: *peer_id,
									trace_id,
									result,
								})
								.unwrap();
						},
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
						OrchestratorToClientEvent::BlockChainEvent { trace_id, event } => {
							tx.send(ProtocolPacket::BlockchainToClient { trace_id, event })
								.await
								.unwrap();
						},
					}
				}
			};

			let _ = tokio::join!(fwd_orchestrator, from_orchestrator);

			log::warn!(target: "dkg", "Communications between orchestrator and DKG client for peer {peer_id:?} died")
		} else {
			panic!("Invalid first packet received from peer")
		}
	}

	async fn orchestrate(self) -> std::io::Result<()> {
		let mut test_cases = self.generate_test_cases();
		let mut client_to_orchestrator_rx = self.orchestrator_rx.lock().await.take().unwrap();
		let mut current_round_completed_count_keygen = 0;
		let mut current_round_completed_count_signing = 0;
		let intra_test_phase = &mut IntraTestPhase::new();

		let cl = self.clients.clone();
		let state = self.orchestrator_state.clone();

		//spawn the watcher task
		tokio::task::spawn(async move {
			let mut interval = tokio::time::interval(Duration::from_secs(5));
			loop {
				interval.tick().await;
				log::info!(target: "dkg", "Orchestrator state is {state:?}", state = state.load(Ordering::SeqCst));
				if state.load(Ordering::SeqCst) != OrchestratorState::AwaitingRoundCompletion {
					continue
				}

				let clients = cl.read().await;
				for (id, client) in clients.iter() {
					if !client.outstanding_tasks_keygen.is_empty() {
						log::warn!(target: "dkg", "Client {id:?} has {tasks:?} outstanding KEYGEN task(s)", tasks = client.outstanding_tasks_keygen.len());
					}

					if !client.outstanding_tasks_signing.is_empty() {
						log::warn!(target: "dkg", "Client {id:?} has {tasks:?} outstanding SIGNING task(s)", tasks = client.outstanding_tasks_signing.values().map(|r| r.len()).sum::<usize>());
					}
				}
			}
		});

		while let Some(client_update) = client_to_orchestrator_rx.recv().await {
			match self.orchestrator_state.load(Ordering::SeqCst) {
				o_state @ OrchestratorState::WaitingForInit => match client_update {
					ClientToOrchestratorEvent::ClientReady => {
						let clients = self.clients.read().await;
						// NOTE: the client automatically puts its handle inside this map. We do not
						// have to here
						if clients.len() == self.config.n_clients {
							// we are ready to begin testing rounds
							std::mem::drop(clients);
							self.orchestrator_begin_next_round(&mut test_cases, intra_test_phase)
								.await;
						}
					},

					c_update => log_invalid_signal(&o_state, &c_update),
				},

				o_state @ OrchestratorState::AwaitingRoundCompletion => {
					match &client_update {
						ClientToOrchestratorEvent::ClientReady =>
							log_invalid_signal(&o_state, &client_update),
						ClientToOrchestratorEvent::TestResult { peer_id, trace_id, result } => {
							let res = match result {
								TestResult::Keygen { result, pub_key } => {
									// set the public key that way other nodes can verify that
									// the public key was submitted
									// TODO: Make sure that we set_next_public key based on input
									self.blockchain.set_pub_key(
										intra_test_phase.round_number(),
										pub_key.clone(),
									);

									result
								},

								TestResult::Sign { result } => result,
							};

							let mut clients = self.clients.write().await;
							let client = clients.get_mut(peer_id).unwrap();

							// regardless of success, increment completed count for the current
							// round
							if matches!(result, TestResult::Keygen { .. }) &&
								matches!(intra_test_phase, IntraTestPhase::Keygen { .. })
							{
								if let Err(err) = res {
									log::error!(target: "dkg", "Peer {peer_id:?} unsuccessfully completed KEYGEN test {trace_id:?}. Reason: {err:?}");
								} else {
									log::info!(target: "dkg", "Peer {peer_id:?} successfully completed KEYGEN test {trace_id:?}");
									client.outstanding_tasks_keygen.remove(trace_id);
								}
								current_round_completed_count_keygen += 1;
							}

							if matches!(result, TestResult::Sign { .. }) &&
								matches!(intra_test_phase, IntraTestPhase::Signing { .. })
							{
								if let Err(err) = res {
									log::error!(target: "dkg", "Peer {peer_id:?} unsuccessfully completed SIGNING test {trace_id:?}. Reason: {err:?}");
								} else {
									log::info!(target: "dkg", "Peer {peer_id:?} successfully completed SIGNING test {trace_id:?}");
									let entry = client
										.outstanding_tasks_signing
										.get_mut(trace_id)
										.expect("Should exist");
									assert!(entry.pop().is_some());
									if entry.is_empty() {
										// remove from map
										client.outstanding_tasks_signing.remove(trace_id);
									}
								}
								current_round_completed_count_signing += 1;
								log::info!(target: "dkg", "RBX {}", current_round_completed_count_signing);
							}
						},
					}

					// at the end, check if the round is complete
					let keygen_complete =
						current_round_completed_count_keygen == self.config.n_clients;

					if keygen_complete && matches!(intra_test_phase, IntraTestPhase::Keygen { .. })
					{
						// keygen is complete, and, we are ready to rotate into either the next
						// session, or the next test phase (i.e., signing)
						if intra_test_phase.unsigned_proposals_count() > 0 {
							// since there are unsigned proposals, we need to keep the current
							// session and begin the signing tests
							intra_test_phase.keygen_to_signing();
							// only do this to create a new block header
							intra_test_phase.increment_round_number();
							self.begin_next_test_print(intra_test_phase).await;
							self.send_finality_notification(intra_test_phase).await;
							continue
						} else {
							// there are no unsigned proposals, so we can move on to the next
							// session
						}
					}

					let signing_complete =
						if self.config.unsigned_proposals_per_session.unwrap_or(0) > 0 {
							let current_round_unsigned_proposals_needed =
								intra_test_phase.unsigned_proposals_count();
							current_round_completed_count_signing ==
								(self.config.threshold + 1) *
									current_round_unsigned_proposals_needed
						} else {
							// pretend signing is complete to move on to the next session/round
							true
						};

					if keygen_complete && signing_complete {
						current_round_completed_count_keygen = 0; // reset to 0 for next round
						current_round_completed_count_signing = 0;
						intra_test_phase.increment_round_number();
						// clear all signing + keygen tests, since t+1 are needed, not n
						self.clear_tasks().await;
						self.orchestrator_begin_next_round(&mut test_cases, intra_test_phase).await
					}
				},
				o_state @ OrchestratorState::Complete =>
					log_invalid_signal(&o_state, &client_update),
			}
		}

		Err(generic_error("client_to_orchestrator_tx's all dropped"))
	}

	async fn clear_tasks(&self) {
		let mut clients = self.clients.write().await;
		clients.values_mut().for_each(|client| client.outstanding_tasks_signing.clear());
		clients.values_mut().for_each(|client| client.outstanding_tasks_keygen.clear());
	}

	fn generate_test_cases(&self) -> VecDeque<TestCase> {
		let mut test_cases = VecDeque::new();

		// add all positive cases to the front
		for _ in 0..self.config.positive_cases {
			test_cases.push_back(TestCase::Valid)
		}

		if let Some(error_cases) = &self.config.error_cases {
			// add all error cases to the back
			for error_case in error_cases {
				for _ in 0..error_case.count {
					test_cases.push_back(TestCase::Invalid(error_case.clone()))
				}
			}
		}

		test_cases
	}

	async fn orchestrator_begin_next_round(
		&self,
		test_cases: &mut VecDeque<TestCase>,
		test_phase: &mut IntraTestPhase,
	) {
		log::info!(target: "dkg", "[Orchestrator] Running next round!");

		if let Some(next_case) = test_cases.pop_front() {
			// the first round will not have any unsigned proposals since we're waiting for keygen
			// otherwise, preload the unsigned proposals
			let round_number = test_phase.round_number();
			let unsigned_proposals = if round_number >= 1 {
				let unsigned_proposals =
					(0..self.config.unsigned_proposals_per_session.unwrap_or(0))
						.map(|idx| {
							// ensure a unique unsigned proposal per session per proposal
							let mut bytes = round_number.to_be_bytes().to_vec();
							bytes.extend_from_slice(&idx.to_be_bytes());
							bytes.extend_from_slice(Uuid::new_v4().as_ref());
							let hash = sha2_256(&bytes);
							UnsignedProposal::testing_dummy(hash.to_vec())
						})
						.collect::<Vec<_>>();
				if !unsigned_proposals.is_empty() {
					Some(unsigned_proposals)
				// self.blockchain.set_unsigned_proposals(unsigned_proposals);
				} else {
					None
				}
			} else {
				None
			};

			// begin the next test
			test_phase.session_init(unsigned_proposals, next_case);
			self.begin_next_test_print(test_phase).await;
			self.orchestrator_set_state(OrchestratorState::AwaitingRoundCompletion);
			// phase 1: send finality notifications to each client
			self.send_finality_notification(test_phase).await;
		} else {
			log::info!(target: "dkg", "Orchestrator has finished running all tests");
			self.orchestrator_set_state(OrchestratorState::Complete);
			let mut exit_code = 0;
			// check to see the final state
			let read = self.clients.read().await;
			for (peer_id, client_state) in &*read {
				let outstanding_tasks_keygen = &client_state.outstanding_tasks_keygen;
				let outstanding_tasks_signing = &client_state.outstanding_tasks_signing;
				// the client should have no outstanding tasks if successful
				let success =
					outstanding_tasks_keygen.is_empty() && outstanding_tasks_signing.is_empty();
				if !success {
					exit_code = 1;
					log::info!(target: "dkg", "Peer {peer_id:?} final state FAILURE | Failed tasks: KEYGEN: {outstanding_tasks_keygen:?}, SIGNING: {outstanding_tasks_signing:?}")
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
			log::info!(target: "dkg", "Orchestrator is exiting with code {exit_code}");
			std::process::exit(exit_code);
		}
	}

	async fn begin_next_test_print(&self, test_phase: &IntraTestPhase) {
		self.logger.clear_checkpoints();
		let test_round = test_phase.round_number();
		let test_phase = match test_phase {
			IntraTestPhase::Keygen { .. } => "KEYGEN",
			IntraTestPhase::Signing { .. } => "SIGNING",
		};

		for x in (1..=3).rev() {
			log::info!(target: "dkg", "[Orchestrator] Beginning next {test_phase} test for session {test_round} in {x}");
			tokio::time::sleep(Duration::from_millis(1000)).await
		}
	}

	fn orchestrator_set_state(&self, state: OrchestratorState) {
		self.orchestrator_state.store(state, Ordering::SeqCst);
	}

	async fn send_finality_notification(&self, test_phase: &mut IntraTestPhase) {
		// phase 1: send finality notifications to each client
		let round_number = test_phase.round_number();
		let trace_id = test_phase.trace_id();
		let next_case = test_phase.test_case().unwrap().clone();

		let mut write = self.clients.write().await;
		let next_finality_notification = create_mocked_finality_blockchain_event(round_number);
		for (_id, client) in write.iter_mut() {
			match test_phase {
				IntraTestPhase::Keygen { trace_id, .. } => {
					client.outstanding_tasks_keygen.insert(*trace_id, next_case.clone());
					// always set the unsigned props to empty to ensure no signing protocol executes
					self.blockchain.set_unsigned_proposals(vec![]);
				},
				IntraTestPhase::Signing { trace_id, queued_unsigned_proposals, .. } => {
					if let Some(unsigned_propos) = queued_unsigned_proposals.clone() {
						for _ in 0..unsigned_propos.len() {
							client
								.outstanding_tasks_signing
								.entry(*trace_id)
								.or_default()
								.push(next_case.clone());
						}
						self.blockchain.set_unsigned_proposals(unsigned_propos);
					}
				},
			}

			// Send out a MockBlockChainEvent
			client
				.orchestrator_to_client_subtask
				.send(OrchestratorToClientEvent::BlockChainEvent {
					trace_id,
					event: next_finality_notification.clone(),
				})
				.unwrap();
		}
	}
}

fn generic_error<T: Into<String>>(err: T) -> std::io::Error {
	std::io::Error::new(std::io::ErrorKind::Other, err.into())
}

fn log_invalid_signal(o_state: &OrchestratorState, c_update: &ClientToOrchestratorEvent) {
	log::error!(target: "dkg", "Orchestrator state is {o_state:?}, yet, the client's update state is {c_update:?}")
}

// Two fields are used by the DKG: the block number via header.number(), and the hash via
// header.hash(). So long as these values remain unique, the DKG should work as expected
fn create_mocked_finality_blockchain_event(block_number: u64) -> MockBlockchainEvent<TestBlock> {
	let header = sp_runtime::generic::Header::<u64, _>::new_from_number(block_number);
	let mut slice = [0u8; 32];
	slice[..8].copy_from_slice(&block_number.to_be_bytes());
	// add random uuid to ensure uniqueness
	slice[8..24].copy_from_slice(&Uuid::new_v4().to_u128_le().to_be_bytes());

	let hash = sp_runtime::testing::H256::from(slice);
	let summary = FinalizeSummary { header, finalized: vec![hash], stale_heads: vec![] };

	let (tx, _rx) = sc_utils::mpsc::tracing_unbounded("mpsc_finality_notification", 999999);
	let notification = FinalityNotification::<TestBlock>::from_summary(summary, tx);
	MockBlockchainEvent::FinalityNotification { notification }
}

pub trait MutableBlockchain: Clone + Send + 'static {
	fn set_unsigned_proposals(
		&self,
		propos: Vec<(UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>, u64)>,
	);
	fn set_pub_key(&self, session: u64, key: Vec<u8>);
}

enum IntraTestPhase {
	Keygen {
		trace_id: Uuid,
		queued_unsigned_proposals:
			Option<Vec<UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>>>,
		round_number: u64,
		test_case: Option<TestCase>,
	},
	Signing {
		trace_id: Uuid,
		queued_unsigned_proposals:
			Option<Vec<(UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>, u64)>>,
		round_number: u64,
		test_case: TestCase,
	},
}

impl IntraTestPhase {
	fn new() -> Self {
		Self::Keygen {
			trace_id: Uuid::new_v4(),
			queued_unsigned_proposals: None,
			round_number: 0,
			test_case: None,
		}
	}

	fn keygen_to_signing(&mut self) {
		if let Self::Keygen { trace_id, queued_unsigned_proposals, round_number, test_case } = self
		{
			let queued_unsigned_proposals = queued_unsigned_proposals.take();
			let mut queued_unsigned_proposals_with_expiry: Vec<(
				UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>,
				u64,
			)> = Default::default();
			for prop in queued_unsigned_proposals.iter() {
				for p in prop.iter() {
					queued_unsigned_proposals_with_expiry.push((p.clone(), 1_u64));
				}
			}
			let test_case = test_case.take().unwrap();
			*self = Self::Signing {
				trace_id: *trace_id,
				queued_unsigned_proposals: Some(queued_unsigned_proposals_with_expiry),
				round_number: *round_number,
				test_case,
			};
		} else {
			panic!("Invalid call to keygen_to_signing")
		}
	}

	fn session_init(
		&mut self,
		unsigned_proposals: Option<
			Vec<UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>>,
		>,
		test_case: TestCase,
	) {
		// rotate signing into keygen, or keygen into keygen if there are no signing tests.
		// Does not increment the round number, as this should be done manually elsewhere
		let round_number = self.round_number();
		*self = Self::Keygen {
			trace_id: Uuid::new_v4(),
			queued_unsigned_proposals: unsigned_proposals,
			round_number,
			test_case: Some(test_case),
		};
	}

	fn increment_round_number(&mut self) {
		match self {
			Self::Keygen { round_number, .. } => *round_number += 1,
			Self::Signing { round_number, .. } => *round_number += 1,
		}
	}

	fn round_number(&self) -> u64 {
		match self {
			Self::Keygen { round_number, .. } => *round_number,
			Self::Signing { round_number, .. } => *round_number,
		}
	}

	fn unsigned_proposals_count(&self) -> usize {
		match self {
			Self::Keygen { queued_unsigned_proposals, .. } =>
				queued_unsigned_proposals.as_ref().map(|v| v.len()).unwrap_or(0),
			Self::Signing { queued_unsigned_proposals, .. } =>
				queued_unsigned_proposals.as_ref().map(|v| v.len()).unwrap_or(0),
		}
	}

	fn trace_id(&self) -> Uuid {
		match self {
			Self::Keygen { trace_id, .. } => *trace_id,
			Self::Signing { trace_id, .. } => *trace_id,
		}
	}

	fn test_case(&self) -> Option<&TestCase> {
		match self {
			Self::Keygen { test_case, .. } => test_case.as_ref(),
			Self::Signing { test_case, .. } => Some(test_case),
		}
	}
}
