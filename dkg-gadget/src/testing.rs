use sc_client_api::BlockchainEvents;
use sp_api::BlockT;
use tokio::net::ToSocketAddrs;

/// When peers use a Client, the streams they receive are suppose
/// to come from the BlockChain. However, for testing purposes, we will mock
/// the blockchain and thus send pseudo notifications to subscribing clients.
/// To allow flexibility in the design, each [`MockClient`] will need to connect
/// to a MockBlockchain service that periodically sends notifications to all connected
/// clients.
pub struct MockClient<B> {}

impl<B: Block> MockClient<B> {
	pub async fn connect<T: ToSocketAddrs>(mock_bc_addr: T) -> std::io::Result<Self> {
		let socket = tokio::net::TcpStream::connect(mock_bc_addr).await?;
		let task = async move {
			let transport = tokio_utils::codec::length_delimited::LengthDelimitedCodec::builder()
				.new_io(socket);

			while let Some(packet) = transport.next().await {
				// Decode the packet. Unwrap because invalid packets
				// should not happen on a mock network
				let decoded = Decode::decode(&mut packet.as_slice()).unwrap();
			}

			panic!("The connection to the MockBlockchain died")
		};
	}
}

impl BlockchainEvents<B: Block> for MockClient {
	fn finality_notification_stream(&self) -> sc_client_api::FinalityNotifications<B> {
		TracingUnboundedReceiver
	}

	fn import_notification_stream(&self) -> sc_client_api::ImportNotifications<B> {}

	fn storage_changes_notification_stream(
		&self,
		filter_keys: Option<&[sc_client_api::StorageKey]>,
		child_filter_keys: Option<
			&[(sc_client_api::StorageKey, Option<Vec<sc_client_api::StorageKey>>)],
		>,
	) -> sp_blockchain::Result<sc_client_api::StorageEventStream<B::Hash>> {
	}
}
