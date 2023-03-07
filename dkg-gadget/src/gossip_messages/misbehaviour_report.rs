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
	gossip_engine::GossipEngineIface,
	storage::misbehaviour_reports::store_aggregated_misbehaviour_reports,
	worker::{DKGWorker, KeystoreExt},
	Client,
};
use codec::Encode;
use dkg_logging::{debug, error};
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, DKGMsgStatus, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::AuthorityId, AggregatedMisbehaviourReports, DKGApi, MisbehaviourType,
};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, Get, NumberFor};

pub(crate) fn handle_misbehaviour_report<B, BE, C, GE, MaxProposalLength>(
	dkg_worker: &DKGWorker<B, BE, C, GE, MaxProposalLength>,
	dkg_msg: DKGMessage<AuthorityId>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength>,
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
			if let Some(round) = dkg_worker.rounds.read().as_ref() {
				msg.session_id == round.session_id
			} else {
				false
			}
		};
		debug!(target: "dkg", "Is main round: {}", is_main_round);
		// Create packed message
		let mut signed_payload = Vec::new();
		signed_payload.extend_from_slice(&match msg.misbehaviour_type {
			MisbehaviourType::Keygen => [0x01],
			MisbehaviourType::Sign => [0x02],
		});
		signed_payload.extend_from_slice(msg.session_id.to_be_bytes().as_ref());
		signed_payload.extend_from_slice(msg.offender.as_ref());
		// Authenticate the message against the current authorities
		let reporter = dkg_worker.authenticate_msg_origin(
			is_main_round,
			authorities.unwrap(),
			&signed_payload,
			&msg.signature,
		)?;
		debug!(target: "dkg", "Reporter: {:?}", reporter);
		// Add new report to the aggregated reports
		let mut lock = dkg_worker.aggregated_misbehaviour_reports.write();
		let reports = lock
			.entry((msg.misbehaviour_type, msg.session_id, msg.offender.clone()))
			.or_insert_with(|| AggregatedMisbehaviourReports {
				misbehaviour_type: msg.misbehaviour_type,
				session_id: msg.session_id,
				offender: msg.offender.clone(),
				reporters: Vec::new(),
				signatures: Vec::new(),
			});
		debug!(target: "dkg", "Reports: {:?}", reports);
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

pub(crate) fn gossip_misbehaviour_report<B, BE, C, GE, MaxProposalLength>(
	dkg_worker: &DKGWorker<B, BE, C, GE, MaxProposalLength>,
	report: DKGMisbehaviourMessage,
) where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength>,
{
	let public = dkg_worker.get_authority_public_key();

	// Create packed message
	let mut payload = Vec::new();
	payload.extend_from_slice(&match report.misbehaviour_type {
		MisbehaviourType::Keygen => [0x01],
		MisbehaviourType::Sign => [0x02],
	});
	payload.extend_from_slice(report.session_id.to_be_bytes().as_ref());
	payload.extend_from_slice(report.offender.as_ref());

	if let Ok(signature) = dkg_worker.key_store.sign(&public.clone(), &payload) {
		let encoded_signature = signature.encode();
		let payload = DKGMsgPayload::MisbehaviourBroadcast(DKGMisbehaviourMessage {
			signature: encoded_signature.clone(),
			..report.clone()
		});

		let status =
			if report.session_id == 0 { DKGMsgStatus::ACTIVE } else { DKGMsgStatus::QUEUED };
		let message = DKGMessage::<AuthorityId> {
			sender_id: public.clone(),
			// We need to gossip this misbehaviour, so no specific recipient.
			recipient_id: None,
			status,
			session_id: report.session_id,
			payload,
		};
		let encoded_dkg_message = message.encode();

		match dkg_worker.key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				dkg_logging::debug!(target: "dkg_gadget::gossip", "💀  (Round: {:?}) Sending Misbehaviour message: ({:?} bytes)", report.session_id, encoded_signed_dkg_message.len());
				if let Err(e) = dkg_worker.keygen_gossip_engine.gossip(signed_dkg_message) {
					dkg_logging::error!(target: "dkg_gadget::gossip", "💀  (Round: {:?}) Failed to gossip misbehaviour message: {:?}", report.session_id, e);
				}
			},
			Err(e) => error!(
				target: "dkg_gadget::gossip",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}

		let mut lock = dkg_worker.aggregated_misbehaviour_reports.write();
		let reports = lock
			.entry((report.misbehaviour_type, report.session_id, report.offender.clone()))
			.or_insert_with(|| AggregatedMisbehaviourReports {
				misbehaviour_type: report.misbehaviour_type,
				session_id: report.session_id,
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

		let reports = (*reports).clone();
		// Try to store reports offchain
		if try_store_offchain(dkg_worker, &reports).is_ok() {
			// remove the report from the queue
			lock.remove(&(report.misbehaviour_type, report.session_id, report.offender));
		}
	} else {
		error!(target: "dkg", "Could not sign public key");
	}
}

pub(crate) fn try_store_offchain<B, BE, C, GE, MaxProposalLength>(
	dkg_worker: &DKGWorker<B, BE, C, GE, MaxProposalLength>,
	reports: &AggregatedMisbehaviourReports<AuthorityId>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength>,
{
	let header = &(dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?);
	// Fetch the current threshold for the DKG. We will use the
	// current threshold to determine if we have enough signatures
	// to submit the next DKG public key.
	let threshold = dkg_worker.get_signature_threshold(header) as usize;
	debug!(target: "dkg", "DKG threshold: {}, reports: {}", threshold, reports.reporters.len());
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
