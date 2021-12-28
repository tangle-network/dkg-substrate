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
	types::Stage,
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
use std::{fs, io::Cursor};

pub struct DKGPersistenceState {
	pub initial_check: bool,
}

impl DKGPersistenceState {
	pub fn new() -> Self {
		Self { initial_check: false }
	}

	pub fn start(&mut self) {
		self.initial_check = true;
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
					*header.number(),
				);

				if local_key.is_some() && offline_stage.is_some() {
					rounds.set_local_key(local_key.as_ref().unwrap().local_key.clone());
					rounds.set_completed_offlinestage(
						offline_stage.as_ref().unwrap().completed_offlinestage.clone(),
					);
					rounds.set_stage(Stage::ManualReady)
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
					*header.number(),
				);

				if queued_local_key.is_some() && queued_offline_stage.is_some() {
					// Restore local key and CompletedofflineStage
					rounds.set_local_key(queued_local_key.as_ref().unwrap().local_key.clone());
					rounds.set_completed_offlinestage(
						queued_offline_stage.as_ref().unwrap().completed_offlinestage.clone(),
					);
					rounds.set_stage(Stage::ManualReady)
				}

				worker.set_next_rounds(rounds)
			}
		}
	}
}

pub(crate) fn should_restart_dkg<B, C, BE>(
	worker: &mut DKGWorker<B, C, BE>,
	header: &B::Header,
) -> (bool, bool)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let rounds = worker.take_rounds();
	let next_rounds = worker.take_next_rounds();

	let should_restart_rounds = {
		if rounds.is_none() {
			true
		} else {
			let stalled = rounds.as_ref().unwrap().has_stalled(*header.number());
			worker.set_rounds(rounds.unwrap());
			stalled
		}
	};

	let should_restart_next_rounds = {
		if next_rounds.is_none() {
			true
		} else {
			let stalled = next_rounds.as_ref().unwrap().has_stalled(*header.number());
			worker.set_next_rounds(next_rounds.unwrap());
			stalled
		}
	};

	(should_restart_rounds, should_restart_next_rounds)
}

pub(crate) fn try_restart_dkg<B, C, BE>(worker: &mut DKGWorker<B, C, BE>, header: &B::Header)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let (restart_rounds, restart_next_rounds) = should_restart_dkg(worker, header);
	let mut local_key_path = None;
	let mut offline_stage_path = None;
	let mut queued_local_key_path = None;
	let mut queued_offline_stage_path = None;

	if worker.base_path.is_some() {
		let base_path = worker.base_path.as_ref().unwrap();
		local_key_path = Some(base_path.join(DKG_LOCAL_KEY_FILE));
		offline_stage_path = Some(base_path.join(DKG_OFFLINE_STAGE_FILE));
		queued_local_key_path = Some(base_path.join(QUEUED_DKG_LOCAL_KEY_FILE));
		queued_offline_stage_path = Some(base_path.join(QUEUED_DKG_OFFLINE_STAGE_FILE));
	}
	let public = worker
		.keystore_ref()
		.authority_id(&worker.keystore_ref().public_keys().unwrap())
		.unwrap_or_else(|| panic!("Halp"));

	let authority_set = worker.get_current_validators();
	let queued_authority_set = worker.get_queued_validators();

	if restart_rounds && authority_set.authorities.contains(&public) {
		debug!(target: "dkg_persistence", "Trying to restart dkg for current validators");

		let threshold = validate_threshold(
			authority_set.authorities.len() as u16,
			worker.get_threshold(header).unwrap(),
		);

		let mut rounds = set_up_rounds(
			&authority_set,
			&public,
			threshold,
			local_key_path,
			offline_stage_path,
			*header.number(),
		);

		let _ = rounds.start_keygen(authority_set.id);

		worker.set_rounds(rounds);
	}

	if restart_next_rounds && queued_authority_set.authorities.contains(&public) {
		debug!(target: "dkg_persistence", "Trying to restart dkg for queued validators");
		let threshold = validate_threshold(
			queued_authority_set.authorities.len() as u16,
			worker.get_threshold(header).unwrap(),
		);

		let mut rounds = set_up_rounds(
			&queued_authority_set,
			&public,
			threshold,
			queued_local_key_path,
			queued_offline_stage_path,
			*header.number(),
		);

		let _ = rounds.start_keygen(queued_authority_set.id);

		worker.set_next_rounds(rounds);
	}
}
