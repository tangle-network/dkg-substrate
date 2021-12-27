use crate::{
	types::dkg_topic,
	utils::{find_index, set_up_rounds, validate_threshold},
	worker::DKGWorker,
	Client,
};

use bincode::deserialize_from;
use codec::Encode;
use dkg_primitives::{
	crypto::AuthorityId,
	keys::{CompletedOfflineStage, LocalKey},
	rounds::MultiPartyECDSARounds,
	types::{DKGBufferedMessage, DKGMessage, DKGMsgPayload, RoundId, Stage},
	utils::{
		StoredLocalKey, StoredOfflineStage, DKG_LOCAL_KEY_FILE, DKG_OFFLINE_STAGE_FILE,
		QUEUED_DKG_LOCAL_KEY_FILE, QUEUED_DKG_OFFLINE_STAGE_FILE,
	},
	DKGPayloadKey,
};
use serde::{Deserialize, Serialize};
use sp_api::BlockT as Block;
use sp_blockchain::Backend;
use std::{fs, io::Cursor};

pub struct DKGPersistenceState {
	pub initial_check: bool,
	pub awaiting_messages: bool,
}

impl DKGPersistenceState {
	pub fn new() -> Self {
		Self { initial_check: false, awaiting_messages: false }
	}

	pub fn is_done(&self) -> bool {
		self.initial_check
	}

	pub fn start(&mut self) {
		self.initial_check = true;
	}
}

pub struct DKGMessageBuffers {
	pub offline: Vec<DKGMessage<AuthorityId, DKGPayloadKey>>,
	pub keygen: Vec<DKGMessage<AuthorityId, DKGPayloadKey>>,
}

impl DKGMessageBuffers {
	pub fn new() -> Self {
		Self { offline: vec![], keygen: vec![] }
	}
}

pub fn buffer_message<B, BE, C>(
	worker: &mut DKGWorker<B, BE, C>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
{
	match msg.payload.clone() {
		DKGMsgPayload::Keygen(_) => {
			// we only need to buffer one message from each node
			let msg_from_authority_exists = worker.msg_buffer.keygen.iter().any(|x| x.id == msg.id);
			if !msg_from_authority_exists {
				worker.msg_buffer.keygen.push(msg.clone());
			}
		},
		DKGMsgPayload::Offline(_) => {
			let msg_from_authority_exists =
				worker.msg_buffer.offline.iter().any(|x| x.id == msg.id);
			if !msg_from_authority_exists {
				worker.msg_buffer.offline.push(msg.clone())
			}
		},
		_ => {},
	}
}

pub fn handle_incoming_buffered_message<B, BE, C>(
	worker: &mut DKGWorker<B, BE, C>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
{
	if !worker.dkg_persistence.awaiting_messages {
		return
	}

	let round_id = msg.round_id;
	let mut rounds = worker.take_rounds();
	let mut next_rounds = worker.take_next_rounds();

	match msg.payload.clone() {
		DKGMsgPayload::BufferedKeyGenMessage(payload) |
		DKGMsgPayload::BufferedOfflineMessage(payload) => {
			worker.dkg_persistence.awaiting_messages = false;
			if rounds.is_some() {
				let inner_rounds = rounds.as_mut().unwrap();
				let inner_round_id = inner_rounds.get_id();
				if round_id == inner_round_id {
					for m in &payload.msg {
						if m.round_id == inner_round_id {
							inner_rounds.handle_incoming(m.payload.clone())
						}
					}
				}
			}

			if next_rounds.is_some() {
				let inner_rounds = next_rounds.as_mut().unwrap();
				let inner_round_id = inner_rounds.get_id();
				if round_id == inner_round_id {
					for m in &payload.msg {
						if m.round_id == inner_round_id {
							inner_rounds.handle_incoming(m.payload.clone())
						}
					}
				}
			}
		},
		_ => {},
	}

	if rounds.is_some() {
		worker.set_rounds(rounds.unwrap())
	}

	if next_rounds.is_some() {
		worker.set_next_rounds(next_rounds.unwrap())
	}
}

pub fn handle_buffered_message_request<B, BE, C>(
	worker: &mut DKGWorker<B, BE, C>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
{
	if worker.dkg_persistence.awaiting_messages {
		return
	}

	let public = worker
		.keystore_ref()
		.authority_id(&self.key_store.public_keys().unwrap())
		.unwrap_or_else(|| panic!("Halp"));

	let round_id = msg.round_id;

	match msg.payload.clone() {
		DKGMsgPayload::RequestBufferedKeyGen => {
			if worker.msg_buffer.keygen.is_empty() {
				return
			}
			let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::BufferedKeyGenMessage(DKGBufferedMessage {
					msg: worker.msg_buffer.keygen.clone(),
				}),
			};

			worker.gossip_engine_ref().lock().gossip_message(
				dkg_topic::<B>(),
				message.encode(),
				true,
			);
		},
		DKGMsgPayload::RequestBufferedOffline => {
			if worker.msg_buffer.offline.is_empty() {
				return
			}
			let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::BufferedOfflineMessage(DKGBufferedMessage {
					msg: worker.msg_buffer.offline.clone(),
				}),
			};

			worker.gossip_engine_ref().lock().gossip_message(
				dkg_topic::<B>(),
				message.encode(),
				true,
			);
		},
		_ => {},
	}
}

pub fn try_resume_dkg<B, BE, C>(worker: &mut DKGWorker<B, BE, C>, header: &B::Header)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
{
	if worker.dkg_persistence.initial_check {
		return
	}

	worker.dkg_persistence.start();

	// A new session is starting, the dkg process will start normally
	if worker.is_new_session(header) {
		return
	}

	if let Some((active, queued)) = worker.validator_set(header) {
		let public = worker
			.keystore_ref()
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));

		let mut local_key = None;
		let mut offline_stage = None;
		let mut queued_local_key = None;
		let mut queued_offline_stage = None;

		if worker.base_path.is_some() {
			let base_path = worker.base_path.as_ref().unwrap();
			let local_key_path = base_path.join(DKG_LOCAL_KEY_FILE);
			let offline_stage_path = base_path.join(DKG_OFFLINE_STAGE_FILE);
			let queued_local_key_path = base_path.join(QUEUED_DKG_LOCAL_KEY_FILE);
			let queued_offline_stage_path = base_path.join(QUEUED_DKG_OFFLINE_STAGE_FILE);

			let offline_stage_serialized = fs::read(offline_stage_path);
			let local_key_serialized = fs::read(local_key_path);
			let queued_offline_stage_serialized = fs::read(queued_offline_stage_path);
			let queued_local_key_serialized = fs::read(queued_local_key_path);

			let round_id = active.id;
			let queued_round_id = queued.id;

			if let Ok(offline_stage_serialized) = offline_stage_serialized {
				let mut reader = Cursor::new(offline_stage_serialized);
				let offline_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredOfflineStage>(&mut reader);

				if let Ok(offline_deserialized) = offline_deserialized {
					if round_id == offline_deserialized.round_id {
						offline_stage = Some(offline_deserialized)
					}
				}
			}

			if let Ok(local_key_serialized) = local_key_serialized {
				let mut reader = Cursor::new(local_key_serialized);
				let localkey_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredLocalKey>(&mut reader);

				if let Ok(localkey_deserialized) = localkey_deserialized {
					if round_id == localkey_deserialized.round_id {
						local_key = Some(localkey_deserialized)
					}
				}
			}

			if let Ok(queued_offline_stage_serialized) = queued_offline_stage_serialized {
				let mut reader = Cursor::new(queued_offline_stage_serialized);
				let queued_offline_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredOfflineStage>(&mut reader);

				if let Ok(queued_offline_deserialized) = queued_offline_deserialized {
					if queued_round_id == queued_offline_deserialized.round_id {
						queued_offline_stage = Some(queued_offline_deserialized)
					}
				}
			}

			if let Ok(queued_local_key_serialized) = queued_local_key_serialized {
				let mut reader = Cursor::new(queued_local_key_serialized);
				let queued_localkey_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredLocalKey>(&mut reader);

				if let Ok(queued_localkey_deserialized) = queued_localkey_deserialized {
					if queued_round_id == queued_localkey_deserialized.round_id {
						queued_local_key = Some(queued_localkey_deserialized)
					}
				}
			}

			if active.authorities.contains(&public) {
				let threshold = validate_threshold(
					active.authorities.len() as u16,
					worker.get_threshold(header).unwrap(),
				);

				let mut rounds = set_up_rounds(
					&active.authorities,
					&public,
					threshold,
					Some(local_key_path),
					Some(offline_stage_path),
				);
				if local_key.is_none() {
					// Send a message requesting buffered keygen messages
					let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
						id: public.clone(),
						round_id: rounds.get_id(),
						payload: DKGMsgPayload::RequestBufferedKeyGen,
					};

					worker.gossip_engine_ref().lock().gossip_message(
						dkg_topic::<B>(),
						message.encode(),
						true,
					);

					worker.dkg_persistence.awaiting_messages = true;
				}

				if local_key.is_some() && offline_stage.is_none() {
					rounds.set_local_key(local_key.as_ref().unwrap().local_key.clone());

					// Send a message requesting buffered offline messages
					let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
						id: public.clone(),
						round_id: rounds.get_id(),
						payload: DKGMsgPayload::RequestBufferedOffline,
					};

					worker.gossip_engine_ref().lock().gossip_message(
						dkg_topic::<B>(),
						message.encode(),
						true,
					);

					worker.dkg_persistence.awaiting_messages = true;
				}

				if local_key.is_some() && offline_stage.is_some() {
					rounds.set_local_key(local_key.as_ref().unwrap().local_key.clone());
					rounds.set_completed_offlinestage(
						offline_stage.as_ref().unwrap().completed_offlinestage.clone(),
					)
				}

				worker.set_rounds(rounds)
			}

			if queued.authorities.contains(&public) {
				let threshold = validate_threshold(
					queued.authorities.len() as u16,
					worker.get_threshold(header).unwrap(),
				);

				let mut rounds = set_up_rounds(
					&queued.authorities,
					&public,
					threshold,
					Some(queued_local_key_path),
					Some(queued_offline_stage_path),
				);
				if queued_local_key.is_none() {
					// Send a message requesting list of messages required to join stalled keygen
					let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
						id: public.clone(),
						round_id: rounds.get_id(),
						payload: DKGMsgPayload::RequestBufferedKeyGen,
					};

					worker.gossip_engine_ref().lock().gossip_message(
						dkg_topic::<B>(),
						message.encode(),
						true,
					);

					worker.dkg_persistence.awaiting_messages = true;
				} else if queued_local_key.is_some() && queued_offline_stage.is_none() {
					rounds.set_local_key(queued_local_key.as_ref().unwrap().local_key.clone());
					// Send a message requesting pending offline stage messages
					let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
						id: public.clone(),
						round_id: rounds.get_id(),
						payload: DKGMsgPayload::RequestBufferedOffline,
					};

					worker.gossip_engine_ref().lock().gossip_message(
						dkg_topic::<B>(),
						message.encode(),
						true,
					);

					worker.dkg_persistence.awaiting_messages = true;
				} else if queued_local_key.is_some() && queued_offline_stage.is_some() {
					// Restore local key and CompletedofflineStage
					rounds.set_local_key(queued_local_key.as_ref().unwrap().local_key.clone());
					rounds.set_completed_offlinestage(
						queued_offline_stage.as_ref().unwrap().completed_offlinestage.clone(),
					)
				}

				worker.set_next_rounds(rounds)
			}
		}
	}
}
