use std::collections::HashSet;
use sc_client_api::Backend;
use sp_core::hashing::sha2_256;
use sp_runtime::traits::Block;
use tokio::sync::mpsc::UnboundedReceiver;
use dkg_primitives::types::{DKGError, DKGMessage, NetworkMsgPayload, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use dkg_runtime_primitives::gossip_messages::DKGKeygenMessage;
use crate::async_protocols::blockchain_interface::BlockchainInterface;
use crate::async_protocols::remote::AsyncProtocolRemote;
use crate::Client;
use crate::dkg_modules::wt_frost::{FrostMessage, NetInterface};
use crate::gossip_engine::GossipEngineIface;
use crate::worker::DKGWorker;

pub mod proto;

pub struct FrostKeygenNetworkWrapper<B, BE, C, GE, BI>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		GE: GossipEngineIface, {
	pub dkg_worker: DKGWorker<B, BE, C, GE>,
	pub remote: AsyncProtocolRemote<C>,
	pub message_receiver: UnboundedReceiver<SignedDKGMessage<AuthorityId>>,
	pub authority_id: AuthorityId,
	pub keygen_protocol_hash: [u8; 32],
	pub received_messages: HashSet<[u8;32]>,
	pub engine: BI
}

impl<B, BE, C, GE, BI: BlockchainInterface> FrostKeygenNetworkWrapper<B, BE, C, GE, BI> {
	pub fn new(dkg_worker: DKGWorker<B, BE, C, GE>, engine: BI, remote: AsyncProtocolRemote<C>, authority_id: AuthorityId, keygen_protocol_hash: [u8; 32]) -> Self {
		let message_receiver = remote
			.rx_keygen_signing
			.lock()
			.take()
			.expect("rx_keygen_signing already taken");

		let received_messages = HashSet::new();

		Self { dkg_worker, engine, remote, message_receiver, authority_id, keygen_protocol_hash, received_messages }
	}
}

impl<B, BE, C, GE, BI: BlockchainInterface> NetInterface for FrostKeygenNetworkWrapper<B, BE, C, GE, BI>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		GE: GossipEngineIface {
	type Error = DKGError;

	async fn next_message(&mut self) -> Result<Option<FrostMessage>, Self::Error> {
		loop {
			let message = self.message_receiver.recv().await?;
			// When we receive a message, it is filtered through the Job Manager, and as such
			// we have these guarantees:
			// * The SSID is correct, the block ID and session ID are acceptable, and the task hash is correct
			// We do not need to check these things here, but we do need to check the signature
			let message = self.engine.verify_signature_against_authorities(message).await?;
			let message_bin = message.payload.payload();
			let message_hash = sha2_256(message_bin);

			if !self.received_messages.insert(message_hash) {
				self.dkg_worker.logger.info("Received duplicate FROST keygen message, ignoring");
				continue;
			}

			// Check to make sure we haven't already received the message
			let deserialized = bincode2::deserialize::<FrostMessage>(message_bin)
				.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
			return Ok(Some(deserialized))
		}
	}

	async fn send_message(&mut self, msg: FrostMessage) -> Result<(), Self::Error> {
		let keygen_msg = bincode2::serialize(&msg)
			.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
		let message = DKGMessage {
			sender_id: self.authority_id.clone(),
			recipient_id: None, // We always gossip in FROST
			payload: NetworkMsgPayload::Keygen(DKGKeygenMessage {
				sender_id: 0, // We do not care to put the sender ID in the message for FROST, since it is already inside the FrostMessage
				keygen_msg,// The Frost Message
				keygen_protocol_hash: self.keygen_protocol_hash,
			}),
			session_id: self.remote.session_id,
			associated_block_id: self.remote.associated_block_id,
			ssid: self.remote.ssid,
		};

		self.engine.sign_and_send_msg(message)
	}
}
