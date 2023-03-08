use crate::{mock_blockchain_config::MockBlockchainConfig, transport::*};
use atomic::Atomic;
use std::{
	collections::HashMap,
	net::SocketAddr,
	sync::{atomic::Ordering, Arc},
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::{mpsc, RwLock},
};
use uuid::Uuid;

// TODO: replace with peer id
type PeerId = Vec<u8>;

#[derive(Clone)]
pub struct MockBlockchain {
	listener: Option<TcpListener>,
	config: MockBlockchainConfig,
	clients: Arc<RwLock<HashMap<PeerId, ConnectedClientState>>>,
	// client sub-tasks communicate with the orchestrator using this sender
	to_orchestator: mpsc::UnboundedSender<InternalStatusUpdate>,
	// the orchestrator receives updates from its client sub-tasks from this receiver
	orchestrator_rx: Option<mpsc::UnboundedReceiver<InternalStatusUpdate>>,
	orchestrator_state: Arc<Atomic<OrchestratorState>>,
}

/// For communicating between the orchestrator task and each spawned client sub-task
#[derive(Debug)]
enum InternalStatusUpdate {
	// Once the client has completed its handshake with the mock blockchain,
	// the client sends this to the orchestrator. The orchestrator will begin
	// running the test cases once "n" peers send this status
	ClientReady { peer_id: PeerId },
    // The orchestrator sends this to tell the receiving client to shutdown
    Halt
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
    Complete
}

struct ConnectedClientState {
	// a map from tracing id => test case. Once the test case passes
	// for the specific client, the test case will be removed from the list
	outstanding_tasks: HashMap<Uuid, crate::TestCase>,
	orchestrator_to_this_task: mpsc::UnboundedSender<InternalStatusUpdate>,
}

impl MockBlockchain {
	pub async fn new(config: MockBlockchainConfig) -> std::io::Result<Self> {
		let mut listener = TcpListener::bind(&config.bind).await?;
		let clients = Arc::new(RwLock::new(HashMap::new()));
		let (to_orchestrator, orchestrator_rx) = mpsc::unbounded();
		let orchestrator_state = Arc::new(Atomic::new(Default::default()));

		Ok(Self {
			listener: Some(listener),
			config,
			clients,
			orchestrator_state,
			to_orchestator,
			orchestrator_rx: Some(orchestrator_rx),
		})
	}

	pub async fn execute(mut self) -> std::io::Result<()> {
		let listener = self.listener.take().unwrap();
		let this_orchestrator = self.clone();

		// the listener task takes client streams and handles them
		let listener_task = async move {
			while let Some((stream, addr)) = listener.accept().await {
				let this = self.clone();
				// spawn a sub-task within this listener task
				let _ = tokio::task::spawn(this.handle_stream(stream, addr));
			}

			Err::<(), _>(generic_error("Listener died"))
		};

		// the orchestrator task is what sends events to each spawned sub-task within the listener
		// task
		let orchestrator_task = this_orchestrator.orchestrate();

		tokio::try_join!(listener_task, orchestrator_task)
	}

	/// For debugging purposes, everything will get unwrapped here
	async fn handle_stream(self, stream: TcpStream, addr: SocketAddr) {
		let (tx, rx) = bind_transport(stream);
		// begin handshake process
		let handshake_packet = ProtocolPacket::InitialHandshake;
		tx.send(handshake_packet).await.unwrap();
		let response = rx.next().await.unwrap();

		if let ProtocolPacket::InitialHandshakeResponse { peer_id } = response {
			log::info!(target: "dkg", "Received handshake response from peer {peer_id:?}");
			let mut write = self.clients.write().await;

			// create a channel for allowing the orchestrator to send this sub-task commands
			let (orchestrator_to_this_task, orchestrator_rx) = mpsc::unbounded();
			let state = ConnectedClientState {
				outstanding_tasks: Default::default(),
				orchestrator_to_this_task,
			};

			if write.insert(peer_id.clone(), state).is_some() {
				// when simulating disconnects, this may happen
				log::warn!(target: "dkg", "Inserted peer {peer_id:?} into the clients map, but, overwrote a previous value")
			}

			std::mem::drop(write);

			// Tell the orchestrator we have established a connection with the client
			self.to_orchestator
				.unbounded_send(InternalStatusUpdate::ClientReady { peer_id: peer_id.clone() })
				.unwrap();

			// Now, run the passive handler that reacts to commands from the orchestrator
			while let Some(orchestrator_command) = orchestrator_rx.recv().await {
                match orchestrator_command {
                    InternalStatusUpdate::Halt => {
                        log::info!(target: "dkg", "Peer {peer_id:?} has been requested to halt");
                        // tell the subscribing client to shutdown the DKG
                        tx.send(ProtocolPacket::Halt).await.unwrap();
                        return;
                    }
                }
			}

			panic!("Orchestrator tx dropped for peer {peer_id:?}")
		} else {
			panic!("Invalid first packet received from peer")
		}
	}

	async fn orchestrate(self) -> std::io::Result<()> {
		let client_to_orchestrator_rx = self.orchestrator_rx.take().unwrap();
		while let Some(client_update) = client_to_orchestrator_rx.recv().await {
			match self.orchestrator_state.load(Ordering::SeqCst) {
				o_state @ OrchestratorState::WaitingForInit => match client_update {
					ClientReady { peer_id } => {
                        let read = self.clients.read().await;
                        if read.len() == self.config.n_clients {
                            // we are ready to begin testing rounds
                            std::mem::drop(read);
                            self.begin_next_round().await;
                        }
                    },

					c_update => {
						log::error!(target: "dkg", "Orchestrator state is {o_state:?}, yet, the client's update state is {c_update:?}")
					},
				},

				OrchestratorState::AwaitingRoundCompletion => {
                    // TODO: handle logic for intra-round updates
                },
			}
		}

		Err(generic_error("client_to_orchestrator_tx's all dropped"))
	}

    fn generate_test_cases() -> Vec<TestCase> {

    }

    async fn begin_next_round(&self, test_cases: &mut Vec<TestCase>) {
        self.orchestrator_set_state(OrchestratorState::AwaitingRoundCompletion);

        if let Some(next_case) = test_cases.pop() {

        } else {
            log::info!(target: "dkg", "Orchestrator has finished running all tests").
            let mut exit_code = 0;
            // check to see the final state
            let read = self.clients.read().await;
            for (peer_id, client_state) in read {
                let outstanding_tasks = &client_state.outstanding_tasks;
                // the client should have no outstanding tasks if successful
                let success = outstanding_tasks.is_empty();
                if !success { 
                    exit_code = 1;
                    log::info!(target: "dkg", "Peer {peer_id:?} final state FAILURE | Failed tasks: {outstanding_tasks:?}")
                } else {
                    log::info!(target: "dkg", "Peer {peer_id:?} SUCCESS!")
                }
                client_state.orchestrator_to_this_task.unbounded_send(InternalStatusUpdate::Halt).unwrap();
            }

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
