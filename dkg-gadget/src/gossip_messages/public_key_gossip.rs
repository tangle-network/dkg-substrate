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
	DKGError, DKGMessage, DKGMsgPayload, DKGPublicKeyMessage, SessionId, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	AggregatedPublicKeys, DKGApi, MaxAuthorities, MaxProposalLength,
};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, Get, Header, NumberFor};
use std::{collections::HashMap, sync::Arc};

pub(crate) async fn handle_public_key_broadcast<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	dkg_msg: DKGMessage<Public>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	// Get authority accounts
	let header = &dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?;
	let current_block_number = *header.number();
	let authorities = dkg_worker
		.validator_set(header)
		.await
		.map(|a| (a.0.authorities, a.1.authorities));
	if authorities.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	if let DKGMsgPayload::PublicKeyBroadcast(msg) = dkg_msg.payload {
		let is_genesis_round = msg.session_id == 0;

		let tag = if is_genesis_round { "CURRENT" } else { "NEXT" };

		dkg_worker
			.logger
			.debug(format!("SESSION {}={tag} | Received public key broadcast", msg.session_id));

		let is_main_round = {
			if let Some(session_id) = dkg_worker.keygen_manager.get_latest_executed_session_id() {
				msg.session_id == session_id
			} else {
				false
			}
		};

		dkg_worker.authenticate_msg_origin(
			is_main_round,
			(
				authorities.clone().expect("Authorities not found!").0.into(),
				authorities.expect("Authorities not found!").1.into(),
			),
			&msg.pub_key,
			&msg.signature,
		)?;

		let key_and_sig = (msg.pub_key, msg.signature);
		let session_id = msg.session_id;

		// Whether this generated key was for genesis or next, we always use the next since
		// the threshold is the same for both.
		let threshold = dkg_worker.get_next_signature_threshold(header).await as usize;

		let mut lock = dkg_worker.aggregated_public_keys.write();
		let aggregated_public_keys = lock.entry(session_id).or_default();

		if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
			aggregated_public_keys.keys_and_signatures.push(key_and_sig);
		}

		dkg_worker.logger.debug(format!(
			"SESSION {}={tag} | Threshold {} | Aggregated pubkeys {} | is_main_round {}",
			msg.session_id,
			threshold,
			aggregated_public_keys.keys_and_signatures.len(),
			is_main_round
		));

		if aggregated_public_keys.keys_and_signatures.len() > threshold {
			store_aggregated_public_keys::<B, BE>(
				&dkg_worker.backend,
				&mut lock,
				is_genesis_round,
				session_id,
				current_block_number,
				&dkg_worker.logger,
			)?;
		} else {
			dkg_worker.logger.debug(format!(
				"SESSION {}={tag}| Need more signatures to submit next DKG public key, needs {} more",
				msg.session_id,
				(threshold + 1) - aggregated_public_keys.keys_and_signatures.len()
			));
		}
	}

	Ok(())
}

pub(crate) fn gossip_public_key<GE>(
	key_store: &DKGKeystore,
	gossip_engine: Arc<GE>,
	aggregated_public_keys: &mut HashMap<SessionId, AggregatedPublicKeys>,
	msg: DKGPublicKeyMessage,
) where
	GE: GossipEngineIface,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
{
	let public = key_store.get_authority_public_key();

	if let Ok(signature) = key_store.sign(&public, &msg.pub_key) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::PublicKeyBroadcast(DKGPublicKeyMessage {
			signature: encoded_signature.clone(),
			..msg.clone()
		});

		let message = DKGMessage::<AuthorityId> {
			associated_block_id: 0, // we don't need to associate this message with a block
			sender_id: public.clone(),
			// we need to gossip the final public key to all parties, so no specific recipient in
			// this case.
			recipient_id: None,
			session_id: msg.session_id,
			payload,
		};
		let encoded_dkg_message = message.encode();

		crate::utils::inspect_outbound("pub_key", encoded_dkg_message.len());

		match key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				if let Err(e) = gossip_engine.gossip(signed_dkg_message) {
					gossip_engine.logger().error(format!("Failed to gossip DKG public key: {e:?}"));
				}
			},
			Err(e) => gossip_engine.logger().error(format!("🕸️  Error signing DKG message: {e:?}")),
		}

		aggregated_public_keys
			.entry(msg.session_id)
			.or_default()
			.keys_and_signatures
			.push((msg.pub_key.clone(), encoded_signature));

		gossip_engine
			.logger()
			.debug(format!("Gossiping local node {public} public key and signature"))
	} else {
		gossip_engine.logger().error("Could not sign public key".to_string());
	}
}
