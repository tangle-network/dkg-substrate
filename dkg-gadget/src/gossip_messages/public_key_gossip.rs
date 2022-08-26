use crate::gossip_engine::GossipEngineIface;
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
// Handles non-dkg messages
use crate::{
	storage::public_keys::store_aggregated_public_keys,
	worker::{DKGWorker, KeystoreExt},
	Client, DKGKeystore,
};
use codec::Encode;
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgPayload, DKGMsgStatus, DKGPublicKeyMessage, RoundId,
	SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	AggregatedPublicKeys, DKGApi,
};
use log::{debug, error};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, Header, NumberFor};
use std::{collections::HashMap, sync::Arc};

pub(crate) fn handle_public_key_broadcast<B, BE, C, GE>(
	dkg_worker: &mut DKGWorker<B, BE, C, GE>,
	dkg_msg: DKGMessage<Public>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
{
	if dkg_worker.rounds.is_none() {
		return Ok(())
	}

	// Get authority accounts
	let header = &dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?;
	let current_block_number = *header.number();
	let authorities = dkg_worker.validator_set(header).map(|a| (a.0.authorities, a.1.authorities));
	if authorities.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	if let DKGMsgPayload::PublicKeyBroadcast(msg) = dkg_msg.payload {
		debug!(target: "dkg", "ROUND {} | Received public key broadcast", msg.round_id);

		let is_main_round = {
			if dkg_worker.rounds.is_some() {
				msg.round_id == dkg_worker.rounds.as_ref().unwrap().round_id
			} else {
				false
			}
		};

		dkg_worker.authenticate_msg_origin(
			is_main_round,
			authorities.unwrap(),
			&msg.pub_key,
			&msg.signature,
		)?;

		let key_and_sig = (msg.pub_key, msg.signature);
		let round_id = msg.round_id;
		let mut lock = dkg_worker.aggregated_public_keys.lock();
		let aggregated_public_keys = lock.entry(round_id).or_default();

		if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
			aggregated_public_keys.keys_and_signatures.push(key_and_sig);
		}
		// Fetch the current threshold for the DKG. We will use the
		// current threshold to determine if we have enough signatures
		// to submit the next DKG public key.
		let threshold = dkg_worker.get_next_signature_threshold(header) as usize;
		log::debug!(
			target: "dkg",
			"ROUND {:?} | Threshold {} | Aggregated pubkeys {}",
			msg.round_id, threshold,
			aggregated_public_keys.keys_and_signatures.len()
		);
		if aggregated_public_keys.keys_and_signatures.len() > threshold {
			store_aggregated_public_keys::<B, C, BE>(
				&dkg_worker.backend,
				&mut *lock,
				is_main_round,
				round_id,
				current_block_number,
			)?;
		}
	}

	Ok(())
}

pub(crate) fn gossip_public_key<B, C, BE, GE>(
	key_store: &DKGKeystore,
	gossip_engine: Arc<GE>,
	aggregated_public_keys: &mut HashMap<RoundId, AggregatedPublicKeys>,
	msg: DKGPublicKeyMessage,
) where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let public = key_store.get_authority_public_key();

	if let Ok(signature) = key_store.sign(&public, &msg.pub_key) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::PublicKeyBroadcast(DKGPublicKeyMessage {
			signature: encoded_signature.clone(),
			..msg.clone()
		});

		let status = if msg.round_id == 0u64 { DKGMsgStatus::ACTIVE } else { DKGMsgStatus::QUEUED };
		let message = DKGMessage::<AuthorityId> {
			id: public.clone(),
			status,
			round_id: msg.round_id,
			payload,
		};
		let encoded_dkg_message = message.encode();

		crate::utils::inspect_outbound("pub_key", encoded_dkg_message.len());

		match key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				if let Err(e) = gossip_engine.gossip(signed_dkg_message) {
					error!(target: "dkg", "Failed to gossip DKG public key: {:?}", e);
				}
			},
			Err(e) => error!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}

		aggregated_public_keys
			.entry(msg.round_id)
			.or_default()
			.keys_and_signatures
			.push((msg.pub_key.clone(), encoded_signature));

		debug!(target: "dkg", "Gossiping local node {} public key and signature", public)
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}
