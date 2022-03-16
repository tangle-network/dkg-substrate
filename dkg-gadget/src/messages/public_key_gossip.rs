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
	storage::public_keys::store_aggregated_public_keys, types::dkg_topic, worker::DKGWorker, Client,
};
use codec::Encode;
use dkg_primitives::{
	crypto::Public,
	types::{DKGError, DKGMessage, DKGMsgPayload, DKGPublicKeyMessage, RoundId, SignedDKGMessage},
};
use dkg_runtime_primitives::{crypto::AuthorityId, AggregatedPublicKeys, DKGApi};
use log::{debug, error, trace};
use sc_client_api::Backend;
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header},
};

pub(crate) fn handle_public_key_broadcast<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	dkg_msg: DKGMessage<Public>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if !dkg_worker.dkg_state.listening_for_pub_key &&
		!dkg_worker.dkg_state.listening_for_active_pub_key
	{
		return Err(DKGError::GenericError {
			reason: "Not listening for public key broadcast".to_string(),
		})
	}

	// Get authority accounts
	let at: BlockId<B> = BlockId::number(dkg_worker.latest_block);
	let authority_accounts = dkg_worker.client.runtime_api().get_authority_accounts(&at).ok();
	if authority_accounts.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	match dkg_msg.payload {
		DKGMsgPayload::PublicKeyBroadcast(msg) => {
			debug!(target: "dkg", "Received public key broadcast");

			let is_main_round = {
				if dkg_worker.dkg_state.curr_rounds.is_some() {
					msg.round_id == dkg_worker.dkg_state.curr_rounds.as_ref().unwrap().get_id()
				} else {
					false
				}
			};

			dkg_worker.authenticate_msg_origin(
				is_main_round,
				authority_accounts.unwrap(),
				&msg.pub_key,
				&msg.signature,
			)?;

			let key_and_sig = (msg.pub_key, msg.signature);
			let round_id = msg.round_id;
			let mut aggregated_public_keys = match dkg_worker.aggregated_public_keys.get(&round_id)
			{
				Some(keys) => keys.clone(),
				None => AggregatedPublicKeys::default(),
			};

			if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
				aggregated_public_keys.keys_and_signatures.push(key_and_sig);
				dkg_worker
					.aggregated_public_keys
					.insert(round_id, aggregated_public_keys.clone());
			}
			// Fetch the current threshold for the DKG. We will use the
			// current threshold to determine if we have enough signatures
			// to submit the next DKG public key.
			let (sig_t, _) =
				dkg_worker.get_thresholds(&dkg_worker.latest_block).unwrap_or_default();
			if aggregated_public_keys.keys_and_signatures.len() >= sig_t.into() {
				store_aggregated_public_keys(
					dkg_worker,
					is_main_round,
					round_id,
					&aggregated_public_keys,
					dkg_worker.latest_block,
				)?;
			}
		},
		_ => {},
	}

	Ok(())
}

pub(crate) fn gossip_public_key<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	public_key: Vec<u8>,
	round_id: RoundId,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let sr25519_public = dkg_worker
		.key_store
		.sr25519_authority_id(&dkg_worker.key_store.sr25519_public_keys().unwrap_or_default())
		.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

	let public = dkg_worker
		.key_store
		.authority_id(&dkg_worker.key_store.public_keys().unwrap_or_default())
		.unwrap_or_else(|| panic!("Could not find an ecdsa key in keystore"));

	if let Ok(signature) = dkg_worker.key_store.sr25519_sign(&sr25519_public, &public_key) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::PublicKeyBroadcast(DKGPublicKeyMessage {
			round_id,
			pub_key: public_key.clone(),
			signature: encoded_signature.clone(),
		});

		let message = DKGMessage::<AuthorityId> { id: public.clone(), round_id, payload };
		let encoded_dkg_message = message.encode();

		match dkg_worker.key_store.sr25519_sign(&sr25519_public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				dkg_worker.gossip_engine.lock().gossip_message(
					dkg_topic::<B>(),
					encoded_signed_dkg_message.clone(),
					true,
				);
			},
			Err(e) => trace!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}

		let mut aggregated_public_keys =
			if dkg_worker.aggregated_public_keys.get(&round_id).is_some() {
				dkg_worker.aggregated_public_keys.get(&round_id).unwrap().clone()
			} else {
				AggregatedPublicKeys::default()
			};

		aggregated_public_keys
			.keys_and_signatures
			.push((public_key.clone(), encoded_signature));

		dkg_worker.aggregated_public_keys.insert(round_id, aggregated_public_keys);

		debug!(target: "dkg", "Gossiping local node  {:?} public key and signature", public)
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}
