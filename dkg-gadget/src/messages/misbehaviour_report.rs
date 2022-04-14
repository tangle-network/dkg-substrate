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
	DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::AuthorityId, AggregatedMisbehaviourReports, DKGApi, MisbehaviourType,
};
use log::{debug, error};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, Header};

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
	let header = &(dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?);
	let authorities = dkg_worker.validator_set(header).map(|a| (a.0.authorities, a.1.authorities));
	if authorities.is_none() {
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
		signed_payload.extend_from_slice(&match msg.misbehaviour_type {
			MisbehaviourType::Keygen => [0x01],
			MisbehaviourType::Sign => [0x02],
		});
		signed_payload.extend_from_slice(msg.round_id.to_be_bytes().as_ref());
		signed_payload.extend_from_slice(msg.offender.as_ref());
		// Authenticate the message against the current authorities
		let reporter = dkg_worker.authenticate_msg_origin(
			is_main_round,
			authorities.unwrap(),
			&signed_payload,
			&msg.signature,
		)?;
		// Add new report to the aggregated reports
		let reports = dkg_worker
			.aggregated_misbehaviour_reports
			.entry((msg.misbehaviour_type, msg.round_id, msg.offender.clone()))
			.or_insert_with(|| AggregatedMisbehaviourReports {
				misbehaviour_type: msg.misbehaviour_type,
				round_id: msg.round_id,
				offender: msg.offender.clone(),
				reporters: Vec::new(),
				signatures: Vec::new(),
			});

		if !reports.reporters.contains(&reporter) {
			reports.reporters.push(reporter);
			reports.signatures.push(msg.signature);
		}

		// Try to store reports offchain
		let reports = reports.clone();
		try_store_offchain(dkg_worker, &reports)?;
	}

	Ok(())
}

pub(crate) fn gossip_misbehaviour_report<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	report: DKGMisbehaviourMessage,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let public = dkg_worker.get_authority_public_key();

	// Create packed message
	let mut payload = Vec::new();
	payload.extend_from_slice(&match report.misbehaviour_type {
		MisbehaviourType::Keygen => [0x01],
		MisbehaviourType::Sign => [0x02],
	});
	payload.extend_from_slice(report.round_id.to_be_bytes().as_ref());
	payload.extend_from_slice(report.offender.as_ref());

	if let Ok(signature) = dkg_worker.key_store.sign(&public.clone(), &payload) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::MisbehaviourBroadcast(DKGMisbehaviourMessage {
			signature: encoded_signature.clone(),
			..report.clone()
		});

		let message =
			DKGMessage::<AuthorityId> { id: public.clone(), round_id: report.round_id, payload };
		let encoded_dkg_message = message.encode();

		match dkg_worker.key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				debug!(target: "dkg", "Gossiping misbehaviour report");
				dkg_worker.gossip_engine.lock().gossip_message(
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

		let reports = dkg_worker
			.aggregated_misbehaviour_reports
			.entry((report.misbehaviour_type, report.round_id, report.offender.clone()))
			.or_insert_with(|| AggregatedMisbehaviourReports {
				misbehaviour_type: report.misbehaviour_type,
				round_id: report.round_id,
				offender: report.offender.clone(),
				reporters: Vec::new(),
				signatures: Vec::new(),
			});

		if reports.reporters.contains(&public) {
			return
		}

		reports.reporters.push(public);
		reports.signatures.push(encoded_signature);

		debug!(target: "dkg", "Gossiping misbehaviour report and signature");

		// Try to store reports offchain
		let reports = reports.clone();
		let _ = try_store_offchain(dkg_worker, &reports);
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}

pub(crate) fn try_store_offchain<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	reports: &AggregatedMisbehaviourReports<AuthorityId>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let header = &(dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?);
	// Fetch the current threshold for the DKG. We will use the
	// current threshold to determine if we have enough signatures
	// to submit the next DKG public key.
	let threshold = dkg_worker.get_signature_threshold(header) as usize;
	match &reports.misbehaviour_type {
		MisbehaviourType::Keygen =>
			if reports.reporters.len() > threshold {
				store_aggregated_misbehaviour_reports(dkg_worker, reports)?;
			},
		MisbehaviourType::Sign =>
			if reports.reporters.len() >= threshold {
				store_aggregated_misbehaviour_reports(dkg_worker, reports)?;
			},
	};

	Ok(())
}
