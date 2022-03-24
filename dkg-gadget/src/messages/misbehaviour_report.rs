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
	storage::misbehaviour_reports::store_aggregated_misbehaviour_reports, types::dkg_topic,
	worker::DKGWorker, Client,
};
use codec::Encode;
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, RoundId, SignedDKGMessage,
};
use dkg_runtime_primitives::{crypto::AuthorityId, AggregatedMisbehaviourReports, DKGApi};
use log::{debug, error, trace};
use sc_client_api::Backend;
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header},
};

pub(crate) fn handle_misbehaviour_report<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	dkg_msg: DKGMessage<AuthorityId>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	// Get authority accounts
	let header = dkg_worker.latest_header.as_ref().ok_or(DKGError::NoHeader)?;
	let at: BlockId<B> = BlockId::hash(header.hash());
	let authority_accounts = dkg_worker.client.runtime_api().get_authority_accounts(&at).ok();
	if authority_accounts.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	if let DKGMsgPayload::MisbehaviourBroadcast(msg) = dkg_msg.payload {
		debug!(target: "dkg", "Received misbehaviour report");

		let is_main_round = {
			if dkg_worker.rounds.is_some() {
				msg.round_id == dkg_worker.rounds.as_ref().unwrap().get_id()
			} else {
				false
			}
		};
		// Create packed message
		let mut signed_payload = Vec::new();
		signed_payload.extend_from_slice(msg.round_id.to_be_bytes().as_ref());
		signed_payload.extend_from_slice(msg.offender.as_ref());
		// Authenticate the message against the current authorities
		let reporter = dkg_worker.authenticate_msg_origin(
			is_main_round,
			authority_accounts.unwrap(),
			&signed_payload,
			&msg.signature,
		)?;
		// Add new report to the aggregated reports
		let round_id = msg.round_id;
		let mut reports = match dkg_worker
			.aggregated_misbehaviour_reports
			.get(&(round_id, msg.offender.clone()))
		{
			Some(r) => r.clone(),
			None => AggregatedMisbehaviourReports {
				round_id,
				offender: msg.offender.clone(),
				reporters: Vec::new(),
				signatures: Vec::new(),
			},
		};

		if !reports.reporters.contains(&reporter) {
			reports.reporters.push(reporter);
			reports.signatures.push(msg.signature);
			dkg_worker
				.aggregated_misbehaviour_reports
				.insert((round_id, msg.offender), reports.clone());
		}

		// Fetch the current threshold for the DKG. We will use the
		// current threshold to determine if we have enough signatures
		// to submit the next DKG public key.
		let threshold = dkg_worker.get_signature_threshold(header) as usize;
		if reports.reporters.len() >= (threshold + 1) {
			store_aggregated_misbehaviour_reports(dkg_worker, &reports)?;
		}
	}

	Ok(())
}

pub(crate) fn gossip_misbehaviour_report<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	offender: dkg_runtime_primitives::crypto::AuthorityId,
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

	// Create packed message
	let mut payload = Vec::new();
	payload.extend_from_slice(round_id.to_be_bytes().as_ref());
	payload.extend_from_slice(offender.as_ref());

	if let Ok(signature) = dkg_worker.key_store.sr25519_sign(&sr25519_public, &payload) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::MisbehaviourBroadcast(DKGMisbehaviourMessage {
			round_id,
			offender: offender.clone(),
			signature: encoded_signature.clone(),
		});

		let message = DKGMessage::<AuthorityId> { id: public, round_id, payload };
		let encoded_dkg_message = message.encode();

		match dkg_worker.key_store.sr25519_sign(&sr25519_public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				dkg_worker.gossip_engine.lock().gossip_message(
					dkg_topic::<B>(),
					encoded_signed_dkg_message,
					true,
				);
			},
			Err(e) => trace!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}

		let mut reports =
			match dkg_worker.aggregated_misbehaviour_reports.get(&(round_id, offender.clone())) {
				Some(reports) => reports.clone(),
				None => AggregatedMisbehaviourReports {
					round_id,
					offender: offender.clone(),
					reporters: Vec::new(),
					signatures: Vec::new(),
				},
			};

		reports.reporters.push(sr25519_public);
		reports.signatures.push(encoded_signature);

		dkg_worker.aggregated_misbehaviour_reports.insert((round_id, offender), reports);
		debug!(target: "dkg", "Gossiping misbehaviour report and signature")
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}
