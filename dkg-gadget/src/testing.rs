use std::marker::PhantomData;

use sc_client_api::{BlockchainEvents, HeaderBackend, AuxStore};
use sp_api::{BlockT, ProvideRuntimeApi};
use tokio::net::ToSocketAddrs;
use dkg_mock_blockchain::TestBlock;
use futures::{StreamExt, SinkExt};
use dkg_mock_blockchain::transport::ProtocolPacket;

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


impl sc_client_api::Backend<TestBlock> for TestBackend {}
impl<GE: GossipEngineIface> HeaderBackend<TestBlock> for MockClient<GE> {}
impl<GE: GossipEngineIface> ProvideRuntimeApi<TestBlock> for MockClient<GE> {}
impl AuxStore for TestBackend {}