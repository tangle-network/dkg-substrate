use crate::{
	async_protocols::{blockchain_interface::BlockchainInterface, remote::AsyncProtocolRemote},
	dkg_modules::wt_frost::{FrostMessage, NetInterface},
	gossip_engine::GossipEngineIface,
	worker::{DKGWorker, ProtoStageType},
	Client,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, DKGMessage, NetworkMsgPayload, SignedDKGMessage, SSID};
use dkg_runtime_primitives::{
	crypto::AuthorityId,
	gossip_messages::{DKGKeygenMessage, DKGOfflineMessage},
	DKGApi, MaxAuthorities, MaxProposalLength,
};
use sc_client_api::Backend;
use sp_core::hashing::sha2_256;
use sp_runtime::traits::{Block, Header, NumberFor};
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedReceiver;

pub mod keygen;
pub mod sign;

pub struct FrostNetworkWrapper<B, BE, C, GE, BI>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	pub dkg_worker: DKGWorker<B, BE, C, GE>,
	pub remote: AsyncProtocolRemote<<<B as Block>::Header as Header>::Number>,
	pub message_receiver: UnboundedReceiver<SignedDKGMessage<AuthorityId>>,
	pub authority_id: AuthorityId,
	pub proto_hash: [u8; 32],
	pub received_messages: HashSet<[u8; 32]>,
	pub engine: BI,
	pub ssid: SSID,
}

impl<B, BE, C, GE, BI: BlockchainInterface> FrostNetworkWrapper<B, BE, C, GE, BI>
where
	B: Block,
	BE: Backend<B> + Unpin,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
	GE: GossipEngineIface,
{
	pub fn new(
		dkg_worker: DKGWorker<B, BE, C, GE>,
		engine: BI,
		remote: AsyncProtocolRemote<<<B as Block>::Header as Header>::Number>,
		authority_id: AuthorityId,
		proto_hash: [u8; 32],
	) -> Self {
		let message_receiver =
			remote.rx_keygen_signing.lock().take().expect("rx_keygen_signing already taken");
		let ssid = remote.ssid;
		let received_messages = HashSet::new();

		Self {
			dkg_worker,
			engine,
			remote,
			message_receiver,
			authority_id,
			proto_hash,
			received_messages,
			ssid,
		}
	}
}

#[async_trait]
impl<B, BE, C, GE, BI: BlockchainInterface> NetInterface for FrostNetworkWrapper<B, BE, C, GE, BI>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	type Error = DKGError;

	async fn next_message(&mut self) -> Result<Option<FrostMessage>, Self::Error> {
		loop {
			if let Some(message) = self.message_receiver.recv().await {
				// When we receive a message, it is filtered through the Job Manager, and as such
				// we have these guarantees:
				// * The SSID is correct, the block ID and session ID are acceptable, and the task
				//   hash is correct
				// We do not need to check these things here, but we do need to check the signature
				match self.engine.verify_signature_against_authorities(message).await {
					Ok(message) => {
						let message_bin = message.payload.payload();
						let message_hash = sha2_256(message_bin);

						// Check to make sure we haven't already received the message
						if !self.received_messages.insert(message_hash) {
							self.dkg_worker
								.logger
								.info("Received duplicate FROST keygen message, ignoring");
							continue
						}

						let deserialized = bincode2::deserialize::<FrostMessage>(message_bin)
							.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
						return Ok(Some(deserialized))
					},
					Err(err) => {
						self.dkg_worker.logger.info(format!(
							"Received invalid FROST keygen message, ignoring: {err:?}"
						));
					},
				}
			} else {
				return Ok(None)
			}
		}
	}

	async fn send_message(&mut self, msg: FrostMessage) -> Result<(), Self::Error> {
		let frost_msg = bincode2::serialize(&msg)
			.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;

		let payload = if matches!(self.remote.proto_stage_type, ProtoStageType::Signing { .. }) {
			NetworkMsgPayload::Offline(DKGOfflineMessage {
				key: vec![],
				signer_set_id: 0,
				offline_msg: frost_msg,
				unsigned_proposal_hash: self.proto_hash,
			})
		} else {
			NetworkMsgPayload::Keygen(DKGKeygenMessage {
				sender_id: 0,          /* We do not care to put the sender ID in the message for
				                        * FROST, since it is already
				                        * inside the FrostMessage */
				keygen_msg: frost_msg, // The Frost Message
				keygen_protocol_hash: self.proto_hash,
			})
		};

		let message = DKGMessage {
			sender_id: self.authority_id.clone(),
			recipient_id: None, // We always gossip/broadcast in FROST
			payload,
			session_id: self.remote.session_id,
			associated_block_id: self.remote.associated_block_id,
			ssid: self.remote.ssid,
		};

		self.engine.sign_and_send_msg(message)
	}
}
