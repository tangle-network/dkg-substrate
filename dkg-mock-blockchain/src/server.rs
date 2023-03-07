use crate::{mock_blockchain_config::MockBlockchainConfig, transport::*};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::RwLock,
};
use uuid::Uuid;

// TODO: replace with peer id
type Key = Vec<u8>;

#[derive(Clone)]
pub struct MockBlockchain {
	listener: Option<TcpListener>,
	config: MockBlockchainConfig,
	clients: Arc<RwLock<HashMap<Vec<u8>, ConnectedClientState>>>,
}

#[derive(Default)]
struct ConnectedClientState {
	// a map from tracing id => test case. Once the test case passes
	// for the specific client, the test case will be removed from the list
	outstanding_tasks: HashMap<Uuid, crate::TestCase>,
}

impl MockBlockchain {
	pub async fn new(config: MockBlockchainConfig) -> std::io::Result<Self> {
		let mut listener = TcpListener::bind(&config.bind).await?;
		let clients = Arc::new(RwLock::new(HashMap::new()));

		Ok(Self { listener: Some(listener), config, clients })
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
			let mut write = self.clients.read().await;

			if write.insert(peer_id.clone(), Default::default).is_some() {
				// when simulating disconnects, this may happen
				log::warn!(target: "dkg", "Inserted peer {peer_id:?} into the clients map, but, overwrote a previous value")
			}

		// TODO: listener to orchestrator commands
		} else {
			panic!("Invalid first packet received from peer")
		}
	}

	async fn orchestrate(self) -> std::io::Result<()> {
		Ok(())
	}
}

fn generic_error<T: Into<String>>(err: T) -> std::io::Error {
	std::io::Error::new(std::io::ErrorKind::Other, err.into())
}
