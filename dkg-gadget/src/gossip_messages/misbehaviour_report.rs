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
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::AuthorityId, AggregatedMisbehaviourReports, DKGApi, MaxAuthorities, MaxProposalLength,
	MaxReporters, MaxSignatureLength, MisbehaviourType,
};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, Get, NumberFor};

pub(crate) async fn handle_misbehaviour_report<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	dkg_msg: DKGMessage<AuthorityId>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	// Get authority accounts
	let header = &(dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?);
	let authorities = dkg_worker
		.validator_set(header)
		.await
		.map(|a| (a.0.authorities, a.1.authorities));
	if authorities.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	if let DKGMsgPayload::MisbehaviourBroadcast(msg) = dkg_msg.payload {
		dkg_worker.logger.debug("Received misbehaviour report".to_string());

		let is_main_round = {
			if let Some(session_id) = dkg_worker.keygen_manager.get_latest_executed_session_id() {
				msg.session_id == session_id
			} else {
				false
			}
		};
		dkg_worker.logger.debug(format!("Is main round: {is_main_round}"));
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
			(
				authorities.clone().expect("Authorities not found!").0.into(),
				authorities.expect("Authorities not found!").1.into(),
			),
			&signed_payload,
			&msg.signature,
		)?;
		dkg_worker.logger.debug(format!("Reporter: {reporter:?}"));
		// Add new report to the aggregated reports
		let reports = {
			let mut lock = dkg_worker.aggregated_misbehaviour_reports.write();
			let reports = lock
				.entry((msg.misbehaviour_type, msg.session_id, msg.offender.clone()))
				.or_insert_with(|| AggregatedMisbehaviourReports {
					misbehaviour_type: msg.misbehaviour_type,
					session_id: msg.session_id,
					offender: msg.offender.clone(),
					reporters: Default::default(),
					signatures: Default::default(),
				});
			dkg_worker.logger.debug(format!("Reports: {reports:?}"));
			if !reports.reporters.contains(&reporter) {
				reports.reporters.try_push(reporter).map_err(|_| DKGError::InputOutOfBounds)?;
				let bounded_signature =
					msg.signature.try_into().map_err(|_| DKGError::InputOutOfBounds)?;
				reports
					.signatures
					.try_push(bounded_signature)
					.map_err(|_| DKGError::InputOutOfBounds)?;
			}

			reports.clone()
		};

		try_store_offchain(dkg_worker, &reports).await?;
	}

	Ok(())
}

pub(crate) async fn gossip_misbehaviour_report<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	report: DKGMisbehaviourMessage,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
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

		let message = DKGMessage::<AuthorityId> {
			associated_block_id: 0,
			sender_id: public.clone(),
			// We need to gossip this misbehaviour, so no specific recipient.
			recipient_id: None,
			session_id: report.session_id,
			payload,
		};
		let encoded_dkg_message = message.encode();

		match dkg_worker.key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();

				dkg_worker.logger.debug(format!(
					"💀  (Round: {:?}) Sending Misbehaviour message: ({:?} bytes)",
					report.session_id,
					encoded_signed_dkg_message.len()
				));
				if let Err(e) = dkg_worker.keygen_gossip_engine.gossip(signed_dkg_message) {
					dkg_worker.logger.error(format!(
						"💀  (Round: {:?}) Failed to gossip misbehaviour message: {:?}",
						report.session_id, e
					));
				}
			},
			Err(e) => dkg_worker.logger.error(format!("🕸️  Error signing DKG message: {e:?}")),
		}

		let reports = {
			let mut lock = dkg_worker.aggregated_misbehaviour_reports.write();
			let reports = lock
				.entry((report.misbehaviour_type, report.session_id, report.offender.clone()))
				.or_insert_with(|| AggregatedMisbehaviourReports {
					misbehaviour_type: report.misbehaviour_type,
					session_id: report.session_id,
					offender: report.offender.clone(),
					reporters: Default::default(),
					signatures: Default::default(),
				});

			if reports.reporters.contains(&public) {
				return Ok(())
			}

			reports.reporters.try_push(public).map_err(|_| DKGError::InputOutOfBounds)?;
			reports
				.signatures
				.try_push(encoded_signature.try_into().map_err(|_| DKGError::InputOutOfBounds)?)
				.map_err(|_| DKGError::InputOutOfBounds)?;

			dkg_worker
				.logger
				.debug("Gossiping misbehaviour report and signature".to_string());

			(*reports).clone()
		};

		// Try to store reports offchain
		if try_store_offchain(dkg_worker, &reports).await.is_ok() {
			// remove the report from the queue
			dkg_worker.aggregated_misbehaviour_reports.write().remove(&(
				report.misbehaviour_type,
				report.session_id,
				report.offender,
			));
		}
		Ok(())
	} else {
		dkg_worker.logger.error("Could not sign public key".to_string());
		Err(DKGError::CannotSign)
	}
}

pub(crate) async fn try_store_offchain<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	reports: &AggregatedMisbehaviourReports<AuthorityId, MaxSignatureLength, MaxReporters>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	let header = &(dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?);
	// Fetch the current threshold for the DKG. We will use the
	// current threshold to determine if we have enough signatures
	// to submit the next DKG public key.
	let threshold = dkg_worker.get_signature_threshold(header).await as usize;
	dkg_worker.logger.debug(format!(
		"DKG threshold: {}, reports: {}",
		threshold,
		reports.reporters.len()
	));
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
