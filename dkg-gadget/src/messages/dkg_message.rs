use std::sync::Arc;
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
use crate::{messages::public_key_gossip::gossip_public_key, persistence::store_localkey, types::dkg_topic, worker::DKGWorker, Client, DKGKeystore};
use codec::Encode;
use dkg_primitives::{
	crypto::Public,
	rounds::MultiPartyECDSARounds,
	types::{DKGError, DKGMessage, DKGPublicKeyMessage, DKGResult, SignedDKGMessage},
	GOSSIP_MESSAGE_RESENDING_LIMIT,
};
use dkg_runtime_primitives::{crypto::AuthorityId, DKGApi};
use log::{error, info, trace};
use parking_lot::Mutex;
use sc_client_api::Backend;
use sc_network_gossip::GossipEngine;
use sp_runtime::traits::{Block, Header, NumberFor};

/// Sends outgoing dkg messages
pub(crate) fn send_outgoing_dkg_messages<B, C, BE>(mut dkg_worker: &mut DKGWorker<B, C, BE>)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let mut keys_to_gossip = Vec::new();
	let mut rounds_send_result = vec![];
	let mut next_rounds_send_result = vec![];

	if let Some(mut rounds) = dkg_worker.rounds.take() {
		let authorities = dkg_worker.current_validator_set.read().authorities.clone();
		if let Some(id) =
			dkg_worker.key_store.authority_id(&authorities)
		{
			rounds_send_result =
				send_messages(dkg_worker, &mut rounds, id, dkg_worker.get_latest_block_number());
		} else {
			error!("No local accounts available. Consider adding one via `author_insertKey` RPC.");
		}

		if dkg_worker.active_keygen_in_progress {
			let is_keygen_finished = rounds.is_keygen_finished();
			if is_keygen_finished {
				dkg_worker.active_keygen_in_progress = false;
				let pub_key = rounds.get_public_key().unwrap().to_bytes(true).to_vec();
				info!(target: "dkg", "🕸️  Genesis DKGs keygen has completed: {:?}", pub_key);
				let round_id = rounds.get_id();
				keys_to_gossip.push((round_id, pub_key));
			}
		}

		dkg_worker.rounds = Some(rounds);
	}

	// Check if a there's a key gen process running for the queued authority set
	if dkg_worker.queued_keygen_in_progress {
		if let Some(id) = dkg_worker
			.key_store
			.authority_id(dkg_worker.queued_validator_set.authorities.as_slice())
		{
			if let Some(mut next_rounds) = dkg_worker.next_rounds.take() {
				next_rounds_send_result = send_messages(
					dkg_worker,
					&mut next_rounds,
					id,
					dkg_worker.get_latest_block_number(),
				);

				let is_keygen_finished = next_rounds.is_keygen_finished();
				if is_keygen_finished {
					dkg_worker.queued_keygen_in_progress = false;
					let pub_key = next_rounds.get_public_key().unwrap().to_bytes(true).to_vec();
					info!(target: "dkg", "🕸️  Queued DKGs keygen has completed: {:?}", pub_key);
					keys_to_gossip.push((next_rounds.get_id(), pub_key));
				}
				dkg_worker.next_rounds = Some(next_rounds);
			}
		} else {
			error!("No local accounts available. Consider adding one via `author_insertKey` RPC.");
		}
	}

	for (round_id, pub_key) in &keys_to_gossip {
		let pub_key_msg = DKGPublicKeyMessage {
			round_id: *round_id,
			pub_key: pub_key.clone(),
			signature: vec![],
		};
		let hash = sp_core::blake2_128(&pub_key_msg.encode());
		let count = *dkg_worker.has_sent_gossip_msg.get(&hash).unwrap_or(&0u8);
		if count > GOSSIP_MESSAGE_RESENDING_LIMIT {
			return
		}
		gossip_public_key(dkg_worker, pub_key_msg);
		dkg_worker.has_sent_gossip_msg.insert(hash, count + 1);
	}

	for res in &rounds_send_result {
		if let Err(err) = res {
			dkg_worker.handle_dkg_error(err.clone());
		}
	}

	for res in &next_rounds_send_result {
		if let Err(err) = res {
			dkg_worker.handle_dkg_error(err.clone());
		}
	}
}

/// send actual messages
fn send_messages<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	rounds: &mut MultiPartyECDSARounds<NumberFor<B>>,
	authority_id: Public,
	at: NumberFor<B>,
) -> Vec<Result<DKGResult, DKGError>>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let results = rounds.proceed(at);

	for result in &results {
		if let Ok(DKGResult::KeygenFinished { round_id, local_key }) = result.clone() {
			let _ = store_localkey(*local_key, round_id, rounds.get_local_key_path(), dkg_worker);
		}
	}

	let id = rounds.get_id();
	let messages = rounds
		.get_outgoing_messages()
		.into_iter()
		.map(|payload| DKGMessage { id: authority_id.clone(), payload, round_id: id.clone() })
		.collect::<Vec<DKGMessage<_>>>();

	sign_and_send_messages(&dkg_worker.gossip_engine, &dkg_worker.key_store,messages);

	results
}

pub(crate) fn sign_and_send_messages<B>(
	gossip_engine: &Arc<Mutex<GossipEngine<B>>>,
	dkg_keystore: &DKGKeystore,
	dkg_messages: impl Into<UnsignedMessages>,
) where
	B: Block
{
	let mut dkg_messages = dkg_messages.into();
	let sr25519_public = dkg_keystore.sr25519_public_key(&dkg_keystore.sr25519_public_keys().unwrap_or_default())
		.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

	let mut engine_lock = gossip_engine.lock();

	while let Some(dkg_message) = dkg_messages.next() {
		match dkg_keystore.sr25519_sign(&sr25519_public, &dkg_message.encode()) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: dkg_message.clone(), signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();
				engine_lock.gossip_message(dkg_topic::<B>(), encoded_signed_dkg_message, true);
			},
			Err(e) => trace!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		};

		trace!(target: "dkg", "🕸️  Sent DKG Message of len {}", dkg_message.encoded_size());
	}
}


pub(crate) enum UnsignedMessages {
	Single(Option<DKGMessage<AuthorityId>>),
	Multiple(Vec<DKGMessage<AuthorityId>>)
}

impl From<DKGMessage<AuthorityId>> for UnsignedMessages {
	fn from(item: DKGMessage<AuthorityId>) -> Self {
		UnsignedMessages::Single(Some(item))
	}
}

impl From<Vec<DKGMessage<AuthorityId>>> for UnsignedMessages {
	fn from(messages: Vec<DKGMessage<AuthorityId>>) -> Self {
		UnsignedMessages::Multiple(messages)
	}
}

impl Iterator for UnsignedMessages {
	type Item = DKGMessage<AuthorityId>;

	fn next(&mut self) -> Option<Self::Item> {
		match self {
			Self::Single(msg) => msg.take(),
			Self::Multiple(messages) => messages.pop()
		}
	}
}
