use crate::{
	types::dkg_topic,
	utils::{find_index, set_up_rounds, validate_threshold},
	worker::DKGWorker,
	Client,
};
use bincode::deserialize_from;
use codec::{Decode, Encode};
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
use dkg_runtime_primitives::DKGApi;
use log::debug;
use sc_client_api::Backend;
use serde::{Deserialize, Serialize};
use sp_api::{BlockT as Block, HeaderT as Header};
use std::{collections::HashMap, fs, io::Cursor};

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
	pub offline: HashMap<RoundId, Vec<Vec<u8>>>,
	pub keygen: HashMap<RoundId, Vec<Vec<u8>>>,
}

impl DKGMessageBuffers {
	pub fn new() -> Self {
		Self { offline: HashMap::new(), keygen: HashMap::new() }
	}
}

pub(crate) fn buffer_message<B, C, BE>(
	worker: &mut DKGWorker<B, C, BE>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	match msg.payload.clone() {
		DKGMsgPayload::Keygen(_) => {
			if worker.msg_buffer.keygen.get(&msg.round_id).is_none() {
				return
			}
			// we only need to buffer one message from each node
			let msg_from_authority_exists =
				worker.msg_buffer.keygen.get(&msg.round_id).unwrap().iter().any(
					|x| match DKGMessage::<AuthorityId, DKGPayloadKey>::decode(&mut &x[..]) {
						Ok(m) => msg.id == m.id,
						_ => false,
					},
				);
			if !msg_from_authority_exists {
				debug!(target: "dkg_persistence", "Buffering keygen message, {:?}", msg);
				let keygen = if worker.msg_buffer.keygen.get_mut(&msg.round_id).is_some() {
					worker.msg_buffer.keygen.get_mut(&msg.round_id).unwrap()
				} else {
					worker.msg_buffer.keygen.insert(msg.round_id, Default::default());
					worker.msg_buffer.keygen.get_mut(&msg.round_id).unwrap()
				};
				keygen.push(msg.encode());
			}
		},
		DKGMsgPayload::Offline(_) => {
			if worker.msg_buffer.offline.get(&msg.round_id).is_none() {
				return
			}
			let msg_from_authority_exists =
				worker.msg_buffer.offline.get(&msg.round_id).unwrap().iter().any(|x| {
					match DKGMessage::<AuthorityId, DKGPayloadKey>::decode(&mut &x[..]) {
						Ok(m) => msg.id == m.id,
						_ => false,
					}
				});
			if !msg_from_authority_exists {
				debug!(target: "dkg_persistence", "Buffering offline message, {:?}", msg);
				let offline = if worker.msg_buffer.offline.get_mut(&msg.round_id).is_some() {
					worker.msg_buffer.offline.get_mut(&msg.round_id).unwrap()
				} else {
					worker.msg_buffer.offline.insert(msg.round_id, Default::default());
					worker.msg_buffer.offline.get_mut(&msg.round_id).unwrap()
				};
				offline.push(msg.encode());
			}
		},
		_ => {},
	}
}

pub(crate) fn handle_incoming_buffered_message<B, C, BE>(
	worker: &mut DKGWorker<B, C, BE>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
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
			debug!(target: "dkg_persistence", "Handling incoming buffered message, {:?}", msg);
			worker.dkg_persistence.awaiting_messages = false;
			if rounds.is_some() {
				let inner_rounds = rounds.as_mut().unwrap();
				let inner_round_id = inner_rounds.get_id();
				if round_id == inner_round_id {
					for m in &payload.msg {
						match DKGMessage::<AuthorityId, DKGPayloadKey>::decode(&mut &m[..]) {
							Ok(msg) =>
								if msg.round_id == inner_round_id {
									let _ = inner_rounds.handle_incoming(msg.payload.clone());
								},
							_ => {},
						}
					}
				}
			}

			if next_rounds.is_some() {
				let inner_rounds = next_rounds.as_mut().unwrap();
				let inner_round_id = inner_rounds.get_id();
				if round_id == inner_round_id {
					for m in &payload.msg {
						match DKGMessage::<AuthorityId, DKGPayloadKey>::decode(&mut &m[..]) {
							Ok(msg) =>
								if msg.round_id == inner_round_id {
									let _ = inner_rounds.handle_incoming(msg.payload.clone());
								},
							_ => {},
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

pub(crate) fn handle_buffered_message_request<B, C, BE>(
	worker: &mut DKGWorker<B, C, BE>,
	msg: DKGMessage<AuthorityId, DKGPayloadKey>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if worker.dkg_persistence.awaiting_messages {
		return
	}

	let public = worker
		.keystore_ref()
		.authority_id(&worker.keystore_ref().public_keys().unwrap())
		.unwrap_or_else(|| panic!("Halp"));

	let round_id = msg.round_id;

	match msg.payload.clone() {
		DKGMsgPayload::RequestBufferedKeyGen => {
			if worker.msg_buffer.keygen.get(&msg.round_id).is_none() {
				return
			}
			if worker.msg_buffer.keygen.get(&msg.round_id).unwrap().is_empty() {
				return
			}
			let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::BufferedKeyGenMessage(DKGBufferedMessage {
					msg: worker.msg_buffer.keygen.get(&msg.round_id).unwrap().clone(),
				}),
			};
			debug!(target: "dkg_persistence", "Responding to buffered keygen message request");
			worker.gossip_engine_ref().lock().gossip_message(
				dkg_topic::<B>(),
				message.encode(),
				true,
			);
		},
		DKGMsgPayload::RequestBufferedOffline => {
			if worker.msg_buffer.offline.get(&msg.round_id).is_none() {
				return
			}
			if worker.msg_buffer.offline.get(&msg.round_id).unwrap().is_empty() {
				return
			}
			let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::BufferedOfflineMessage(DKGBufferedMessage {
					msg: worker.msg_buffer.offline.get(&msg.round_id).unwrap().clone(),
				}),
			};
			debug!(target: "dkg_persistence", "Responding to buffered offline message request");
			worker.gossip_engine_ref().lock().gossip_message(
				dkg_topic::<B>(),
				message.encode(),
				true,
			);
		},
		_ => {},
	}
}

pub(crate) fn try_resume_dkg<B, C, BE>(worker: &mut DKGWorker<B, C, BE>, header: &B::Header)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if worker.dkg_persistence.initial_check {
		return
	}

	worker.dkg_persistence.start();

	// A new session is starting, the dkg process will start normally
	if worker.is_new_session(header) {
		return
	}

	debug!(target: "dkg_persistence", "Trying to resume dkg");
	if let Some((active, queued)) = worker.validator_set(header) {
		let public = worker
			.keystore_ref()
			.authority_id(&worker.keystore_ref().public_keys().unwrap())
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

			let offline_stage_serialized = fs::read(offline_stage_path.clone());
			let local_key_serialized = fs::read(local_key_path.clone());
			let queued_offline_stage_serialized = fs::read(queued_offline_stage_path.clone());
			let queued_local_key_serialized = fs::read(queued_local_key_path.clone());

			let round_id = active.id;
			let queued_round_id = queued.id;

			if let Ok(offline_stage_serialized) = offline_stage_serialized {
				let reader = Cursor::new(offline_stage_serialized);
				let offline_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredOfflineStage>(reader);

				if let Ok(offline_deserialized) = offline_deserialized {
					if round_id == offline_deserialized.round_id {
						offline_stage = Some(offline_deserialized)
					}
				}
			}

			if let Ok(local_key_serialized) = local_key_serialized {
				let reader = Cursor::new(local_key_serialized);
				let localkey_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredLocalKey>(reader);

				if let Ok(localkey_deserialized) = localkey_deserialized {
					if round_id == localkey_deserialized.round_id {
						local_key = Some(localkey_deserialized)
					}
				}
			}

			if let Ok(queued_offline_stage_serialized) = queued_offline_stage_serialized {
				let reader = Cursor::new(queued_offline_stage_serialized);
				let queued_offline_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredOfflineStage>(reader);

				if let Ok(queued_offline_deserialized) = queued_offline_deserialized {
					if queued_round_id == queued_offline_deserialized.round_id {
						queued_offline_stage = Some(queued_offline_deserialized)
					}
				}
			}

			if let Ok(queued_local_key_serialized) = queued_local_key_serialized {
				let reader = Cursor::new(queued_local_key_serialized);
				let queued_localkey_deserialized =
					deserialize_from::<Cursor<Vec<u8>>, StoredLocalKey>(reader);

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
					&active,
					&public,
					threshold,
					Some(local_key_path),
					Some(offline_stage_path),
				);
				if local_key.is_none() {
					debug!(target: "dkg_persistence", "Requesting buffered keygen messages");
					// Send a message requesting buffered keygen messages
					let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
						id: public.clone(),
						round_id: rounds.get_id(),
						payload: DKGMsgPayload::RequestBufferedKeyGen,
					};

					let _ = rounds.start_keygen(rounds.get_id());
					rounds.proceed();
					worker.gossip_engine_ref().lock().gossip_message(
						dkg_topic::<B>(),
						message.encode(),
						true,
					);

					worker.dkg_persistence.awaiting_messages = true;
				}

				if local_key.is_some() && offline_stage.is_none() {
					rounds.set_local_key(local_key.as_ref().unwrap().local_key.clone());
					let s_l = (1..=rounds.dkg_params().2).collect();
					let _ = rounds.reset_signers(rounds.get_id(), s_l);
					rounds.proceed();
					// Send a message requesting buffered offline messages
					debug!(target: "dkg_persistence", "Requesting buffered offline messages");
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
					&queued,
					&public,
					threshold,
					Some(queued_local_key_path),
					Some(queued_offline_stage_path),
				);
				if queued_local_key.is_none() {
					let _ = rounds.start_keygen(rounds.get_id());
					rounds.proceed();
					// Send a message requesting list of messages required to join stalled keygen
					debug!(target: "dkg_persistence", "Requesting buffered queued keygen messages");
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
					debug!(target: "dkg_persistence", "Requesting buffered queued offline keygen messages");
					let s_l = (1..=rounds.dkg_params().2).collect();
					let _ = rounds.reset_signers(rounds.get_id(), s_l);
					rounds.proceed();
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
