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
use crate::{types::dkg_topic, worker::KeystoreExt, DKGKeystore};
use codec::Encode;
use dkg_primitives::types::{DKGMessage, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use log::trace;
use parking_lot::Mutex;
use sc_network_gossip::GossipEngine;
use sp_runtime::traits::Block;

pub(crate) fn sign_and_send_messages<B>(
	gossip_engine: &Arc<Mutex<GossipEngine<B>>>,
	dkg_keystore: &DKGKeystore,
	dkg_messages: impl Into<UnsignedMessages>,
) where
	B: Block,
{
	let dkg_messages = dkg_messages.into();
	let public = dkg_keystore.get_authority_public_key();

	let mut engine_lock = gossip_engine.lock();

	for dkg_message in dkg_messages {
		match dkg_keystore.sign(&public, &dkg_message.encode()) {
			Ok(sig) => {
				let ty = dkg_message.payload.get_type();
				let signed_dkg_message =
					SignedDKGMessage { msg: dkg_message.clone(), signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				crate::utils::inspect_outbound(ty, encoded_signed_dkg_message.len());

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
	Multiple(Vec<DKGMessage<AuthorityId>>),
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
			Self::Multiple(messages) => messages.pop(),
		}
	}
}
