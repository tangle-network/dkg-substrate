// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use crate::{
	utils::{fetch_public_key, fetch_sr25519_public_key, set_up_rounds, validate_threshold},
	worker::DKGWorker,
	Client,
};
use dkg_primitives::{
	crypto::AuthorityId,
	rounds::LocalKey,
	serde_json,
	types::RoundId,
	utils::{
		decrypt_data, encrypt_data, select_random_set, StoredLocalKey, DKG_LOCAL_KEY_FILE,
		QUEUED_DKG_LOCAL_KEY_FILE,
	},
};
use dkg_runtime_primitives::{
	DKGThresholds,
	offchain::crypto::{Pair as AppPair, Public},
	DKGApi,
};
use log::debug;
use sc_client_api::Backend;
use sp_api::{BlockT as Block, HeaderT as Header};
use sp_core::Pair;
use std::{
	fs,
	io::{Error, ErrorKind},
	path::PathBuf,
};

use curv::elliptic::curves::Secp256k1;

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

pub(crate) fn store_localkey<B, C, BE>(
	key: LocalKey<Secp256k1>,
	round_id: RoundId,
	path: Option<PathBuf>,
	worker: &mut DKGWorker<B, C, BE>,
) -> std::io::Result<()>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if let Some(path) = path {
		if let Some(local_keystore) = worker.local_keystore.clone() {
			debug!(target: "dkg_persistence", "Storing local key for {:?}", &path);
			let key_pair = local_keystore.as_ref().key_pair::<AppPair>(
				&Public::try_from(&fetch_sr25519_public_key(worker).0[..])
					.unwrap_or_else(|_| panic!("Could not find keypair in local key store")),
			);

			if let Ok(Some(key_pair)) = key_pair {
				let secret_key = key_pair.to_raw_vec();

				let stored_local_key = StoredLocalKey { round_id, local_key: key };
				let serialized_data = serde_json::to_string(&stored_local_key)
					.map_err(|_| Error::new(ErrorKind::Other, "Serialization failed"))?;

				let encrypted_data = encrypt_data(serialized_data.into_bytes(), secret_key)
					.map_err(|e| Error::new(ErrorKind::Other, e))?;
				fs::write(path.clone(), &encrypted_data[..])?;

				debug!(target: "dkg_persistence", "Successfully stored local key for {:?}", &path);
				Ok(())
			} else {
				Err(Error::new(
					ErrorKind::Other,
					"Local key pair doesn't exist for sr25519 key".to_string(),
				))
			}
		} else {
			Err(Error::new(ErrorKind::Other, "Local keystore doesn't exist".to_string()))
		}
	} else {
		Err(Error::new(ErrorKind::Other, "Path not defined".to_string()))
	}
}

/// We only try to resume the dkg once, if we can find any data for the completed offline stage for
/// the current round
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

	let (active, queued) = worker.validator_set(header);
	if active.authorities.is_empty() || queued.authorities.is_empty() {
		return
	}

	worker.dkg_persistence.start();

	debug!(target: "dkg_persistence", "Trying to restore key gen data");

	let public = fetch_public_key(worker);
	let sr25519_public = fetch_sr25519_public_key(worker);

	let mut local_key = None;
	let mut queued_local_key = None;

	if worker.base_path.is_some() {
		let base_path = worker.base_path.as_ref().unwrap();
		let local_key_path = base_path.join(DKG_LOCAL_KEY_FILE);
		let queued_local_key_path = base_path.join(QUEUED_DKG_LOCAL_KEY_FILE);

		let local_key_serialized = fs::read(local_key_path.clone());
		let queued_local_key_serialized = fs::read(queued_local_key_path.clone());

		let round_id = active.id;
		let queued_round_id = queued.id;

		if worker.local_keystore.is_none() {
			debug!(target: "dkg_persistence", "Exiting no local key store found");
			return
		}

		if let Ok(local_key_serialized) = local_key_serialized {
			let key_pair = worker
				.local_keystore
				.as_ref()
				.unwrap()
				.key_pair::<AppPair>(&Public::try_from(&sr25519_public.0[..]).unwrap());
			if let Ok(Some(key_pair)) = key_pair {
				let decrypted_data = decrypt_data(local_key_serialized, key_pair.to_raw_vec());
				if let Ok(decrypted_data) = decrypted_data {
					debug!(target: "dkg", "Decrypted local key successfully");
					let localkey_deserialized =
						serde_json::from_slice::<StoredLocalKey>(&decrypted_data[..]);

					match localkey_deserialized {
						Ok(localkey_deserialized) => {
							debug!(target: "dkg", "Recovered local key");
							// If the current round_id is not the same as the one found in the
							// stored file then the stored data is invalid
							if round_id == localkey_deserialized.round_id {
								local_key = Some(localkey_deserialized)
							}
						},
						Err(e) => {
							debug!(target: "dkg", "Deserialization failed for local key {:?}", e);
						},
					}
				} else {
					debug!(target: "dkg", "Failed to decrypt local key");
				}
			}
		} else {
			debug!(target: "dkg", "Failed to read local key file {:?}", local_key_path.clone());
		}

		if let Ok(queued_local_key_serialized) = queued_local_key_serialized {
			let key_pair = worker
				.local_keystore
				.as_ref()
				.unwrap()
				.key_pair::<AppPair>(&Public::try_from(&sr25519_public.0[..]).unwrap());
			if let Ok(Some(key_pair)) = key_pair {
				let decrypted_data =
					decrypt_data(queued_local_key_serialized, key_pair.as_ref().to_raw_vec());

				if let Ok(decrypted_data) = decrypted_data {
					let queued_localkey_deserialized =
						serde_json::from_slice::<StoredLocalKey>(&decrypted_data[..]);

					if let Ok(queued_localkey_deserialized) = queued_localkey_deserialized {
						if queued_round_id == queued_localkey_deserialized.round_id {
							queued_local_key = Some(queued_localkey_deserialized)
						}
					}
				}
			}
		}

		if active.authorities.contains(&public) {
			let thresholds = worker.get_thresholds(header.number()).unwrap_or_default();

			let mut maybe_rounds = set_up_rounds(
				&active,
				&public,
				Some(local_key_path),
				&worker.get_authority_reputations(header, active.authorities.clone()),
				thresholds,
			);

			if let Some(mut rounds) = maybe_rounds {
				if local_key.is_some() {
					debug!(target: "dkg_persistence", "Local key set");
					rounds.set_local_key(local_key.as_ref().unwrap().local_key.clone());
					// We create a deterministic signer set using the public key as a seed to the
					// random number generator We need a 32 byte seed, the compressed public key is
					// 33 bytes
					let seed =
						&local_key.as_ref().unwrap().local_key.clone().public_key().to_bytes(true)[1..];
	
					// Signers are chosen from ids used in Keygen phase starting from 1 to n
					// inclusive
					let set = (1..=rounds.dkg_params().2).collect::<Vec<_>>();
					let signers_set = select_random_set(seed, set, rounds.dkg_params().1 + 1);
					if let Ok(signers_set) = signers_set {
						rounds.set_signers(signers_set);
					}
					worker.dkg_state.curr_rounds = Some(rounds);
				}
			}
		}

		if queued.authorities.contains(&public) {
			let thresholds = worker.get_thresholds(header.number()).unwrap_or_default();

			let mut maybe_rounds = set_up_rounds(
				&queued,
				&public,
				Some(queued_local_key_path),
				&worker.get_authority_reputations(header, queued.authorities.clone()),
				thresholds,
			);

			if let Some(mut rounds) = maybe_rounds {
				if queued_local_key.is_some() {
					rounds.set_local_key(queued_local_key.as_ref().unwrap().local_key.clone());
					// We set the signer set using the public key as a seed to select signers at
					// random We need a 32byte seed, the compressed public key is 32 bytes
					let seed = &queued_local_key
						.as_ref()
						.unwrap()
						.local_key
						.clone()
						.public_key()
						.to_bytes(true)[1..];
	
					// Signers are chosen from ids used in Keygen phase starting from 1 to n
					// inclusive
					let set = (1..=rounds.dkg_params().2).collect::<Vec<_>>();
					let signers_set = select_random_set(seed, set, rounds.dkg_params().1 + 1);
					if let Ok(signers_set) = signers_set {
						rounds.set_signers(signers_set);
					}
					worker.dkg_state.next_rounds = Some(rounds);
				}
			}
		}
	}
}

/// To determine if the protocol should be restarted, we check if the
/// protocol is stuck at the keygen stage
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
	let rounds = worker.dkg_state.curr_rounds.take();
	let next_rounds = worker.dkg_state.next_rounds.take();
	worker.get_time_to_restart(header.number());

	let should_restart_rounds = {
		if rounds.is_none() {
			true
		} else {
			let stalled = rounds.as_ref().unwrap().has_stalled();
			worker.dkg_state.curr_rounds = rounds;
			stalled
		}
	};

	let should_restart_next_rounds = {
		if next_rounds.is_none() {
			true
		} else {
			let stalled = next_rounds.as_ref().unwrap().has_stalled();
			worker.dkg_state.next_rounds = next_rounds;
			stalled
		}
	};

	(should_restart_rounds, should_restart_next_rounds)
}

/// If we ascertain that the protocol has stalled and we are part of the current authority set or
/// queued authority set We restart the protocol on our end
pub(crate) fn try_restart_dkg<B, C, BE>(worker: &mut DKGWorker<B, C, BE>, header: &B::Header)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let (restart_rounds, restart_next_rounds) = should_restart_dkg(worker, header);
	let mut local_key_path = None;
	let mut queued_local_key_path = None;

	if worker.base_path.is_some() {
		let base_path = worker.base_path.as_ref().unwrap();
		local_key_path = Some(base_path.join(DKG_LOCAL_KEY_FILE));
		queued_local_key_path = Some(base_path.join(QUEUED_DKG_LOCAL_KEY_FILE));
	}
	let public = fetch_public_key(worker);
	let sr25519_public = fetch_sr25519_public_key(worker);

	// let authority_set = worker.current_validator_set.clone();
	// let queued_authority_set = worker.queued_validator_set.clone();

	let latest_block_num = *header.number();
	if restart_rounds && worker.current_validator_set.authorities.contains(&public) {
		debug!(target: "dkg_persistence", "Trying to restart dkg for current validators");

		let thresholds = worker.get_thresholds(header.number()).unwrap_or_default();

		let mut rounds = set_up_rounds(
			&worker.current_validator_set,
			&public,
			local_key_path,
			&worker.get_authority_reputations(header, worker.current_validator_set.authorities.clone()),
			thresholds,
		);

		worker.active_keygen_in_progress = true;
		worker.dkg_state.listening_for_active_pub_key = true;
		worker.dkg_state.curr_rounds = rounds;

		if worker.dkg_state.curr_rounds.is_some() {
			let _ = worker.dkg_state.curr_rounds.as_mut().unwrap().start_keygen(latest_block_num);
		}
	}

	if restart_next_rounds && worker.queued_validator_set.authorities.contains(&public) {
		debug!(target: "dkg_persistence", "Trying to restart dkg for queued validators");
		let thresholds = worker.get_thresholds(header.number()).unwrap_or_default();
		let mut rounds = set_up_rounds(
			&worker.queued_validator_set,
			&public,
			queued_local_key_path,
			&worker.get_authority_reputations(header, worker.queued_validator_set.authorities.clone()),
			thresholds,
		);

		worker.queued_keygen_in_progress = true;
		worker.dkg_state.listening_for_pub_key = true;
		worker.dkg_state.next_rounds = rounds;
		
		if worker.dkg_state.next_rounds.is_some() {
			let _ = worker.dkg_state.next_rounds.as_mut().unwrap().start_keygen(latest_block_num);
		}
	}
}
