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
	DKGError, DKGMessage, DKGMsgStatus, NetworkMsgPayload, SessionId, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	gossip_messages::PublicKeyMessage,
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

	if let NetworkMsgPayload::PublicKeyBroadcast(msg) = dkg_msg.payload {
		dkg_worker
			.logger
			.debug(format!("SESSION {} | Received public key broadcast", msg.session_id));

		let is_main_round = {
			if let Some(rounds) = dkg_worker.rounds.read().as_ref() {
				msg.session_id == rounds.session_id
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

		// Fetch the current threshold for the DKG. We will use the
		// current threshold to determine if we have enough signatures
		// to submit the next DKG public key.
		let threshold = dkg_worker.get_next_signature_threshold(header).await as usize;

		let mut lock = dkg_worker.aggregated_public_keys.write();
		let aggregated_public_keys = lock.entry(session_id).or_default();

		if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
			aggregated_public_keys.keys_and_signatures.push(key_and_sig);
		}

		dkg_worker.logger.debug(format!(
			"SESSION {} | Threshold {} | Aggregated pubkeys {}",
			msg.session_id,
			threshold,
			aggregated_public_keys.keys_and_signatures.len()
		));

		if aggregated_public_keys.keys_and_signatures.len() > threshold {
			store_aggregated_public_keys::<B, C, BE, MaxProposalLength>(
				&dkg_worker.backend,
				&mut lock,
				is_main_round,
				session_id,
				current_block_number,
				&dkg_worker.logger,
			)?;
		} else {
			dkg_worker.logger.debug(format!(
				"SESSION {} | Need more signatures to submit next DKG public key, needs {} more",
				msg.session_id,
				(threshold + 1) - aggregated_public_keys.keys_and_signatures.len()
			));
		}
	}

	Ok(())
}

pub(crate) fn gossip_public_key<B, C, BE, GE>(
	key_store: &DKGKeystore,
	gossip_engine: Arc<GE>,
	aggregated_public_keys: &mut HashMap<SessionId, AggregatedPublicKeys>,
	msg: PublicKeyMessage,
) where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	C::Api: DKGApi<
		B,
		AuthorityId,
		<<B as Block>::Header as Header>::Number,
		MaxProposalLength,
		MaxAuthorities,
	>,
{
	let public = key_store.get_authority_public_key();

	if let Ok(signature) = key_store.sign(&public, &msg.pub_key) {
		let encoded_signature = signature.encode();
		let payload = NetworkMsgPayload::PublicKeyBroadcast(PublicKeyMessage {
			signature: encoded_signature.clone(),
			..msg.clone()
		});

		let status =
			if msg.session_id == 0u64 { DKGMsgStatus::ACTIVE } else { DKGMsgStatus::QUEUED };
		let message = DKGMessage::<AuthorityId> {
			associated_block_id: 0, // we don't need to associate this message with a block
			sender_id: public.clone(),
			// we need to gossip the final public key to all parties, so no specific recipient in
			// this case.
			recipient_id: None,
			status,
			session_id: msg.session_id,
			retry_id: 0,
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
