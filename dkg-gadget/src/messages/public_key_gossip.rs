use std::collections::HashMap;
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
use crate::{storage::public_keys::store_aggregated_public_keys, types::dkg_topic, worker::DKGWorker, Client, DKGKeystore};
use codec::Encode;
use dkg_primitives::{
	crypto::Public,
	types::{DKGError, DKGMessage, DKGMsgPayload, DKGPublicKeyMessage, SignedDKGMessage},
};
use dkg_runtime_primitives::{crypto::AuthorityId, AggregatedPublicKeys, DKGApi};
use log::{debug, error};
use sc_client_api::Backend;
use sc_network_gossip::GossipEngine;
use sp_runtime::traits::{Block, Header};
use dkg_primitives::types::RoundId;
use crate::worker::KeystoreExt;

pub(crate) fn gossip_public_key<B, C, BE>(
	key_store: &DKGKeystore,
	gossip_engine: &mut GossipEngine<B>,
	aggregated_public_keys: &mut HashMap<RoundId, AggregatedPublicKeys>,
	msg: DKGPublicKeyMessage,
) where
	B: Block,
	BE: Backend<B>,
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

		let message =
			DKGMessage::<AuthorityId> { id: public.clone(), round_id: msg.round_id, payload };
		let encoded_dkg_message = message.encode();

		match key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				gossip_engine.gossip_message(
					dkg_topic::<B>(),
					encoded_signed_dkg_message,
					true,
				);
			},
			Err(e) => error!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}

		aggregated_public_keys.entry(msg.round_id)
			.or_default()
			.keys_and_signatures
			.push((msg.pub_key.clone(), encoded_signature));

		debug!(target: "dkg", "Gossiping local node {} public key and signature", public)
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}
