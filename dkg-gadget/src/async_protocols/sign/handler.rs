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

use curv::{arithmetic::Converter, elliptic::curves::Secp256k1, BigInt};
use dkg_runtime_primitives::UnsignedProposal;
use futures::{stream::FuturesUnordered, StreamExt, TryStreamExt};

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey,
	sign::{CompletedOfflineStage, OfflineStage, PartialSignature, SignManual},
};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::broadcast::Receiver;

use crate::async_protocols::{
	blockchain_interface::BlockchainInterface, get_party_session_id,
	incoming::IncomingAsyncProtocolWrapper, new_inner, remote::MetaHandlerStatus,
	state_machine::StateMachineHandler, AsyncProtocolParameters, BatchKey, GenericAsyncHandler,
	PartyIndex, ProtocolType, Threshold,
};
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgPayload, DKGMsgStatus, DKGVoteMessage, SignedDKGMessage,
};
use dkg_runtime_primitives::crypto::Public;
use futures::FutureExt;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::verify,
	state_machine::sign::SignError::{CompleteSigning, LocalSigning},
};

impl<'a, Out: Send + Debug + 'a> GenericAsyncHandler<'a, Out>
where
	(): Extend<Out>,
{
	/// Top level function for setting up signing
	pub fn setup_signing<BI: BlockchainInterface + 'a>(
		params: AsyncProtocolParameters<BI>,
		threshold: u16,
		unsigned_proposals: Vec<UnsignedProposal>,
		signing_set: Vec<u16>,
		async_index: u8,
	) -> Result<GenericAsyncHandler<'a, ()>, DKGError> {
		assert!(
			threshold + 1 == signing_set.len() as u16,
			"Signing set must be of size threshold + 1"
		);
		let status_handle = params.handle.clone();
		let mut stop_rx =
			status_handle.stop_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let start_rx =
			status_handle.start_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let protocol = async move {
			let maybe_local_key = params.local_key.clone();
			let (keygen_id, _b, _c) = get_party_session_id(&params);
			if let (Some(keygen_id), Some(local_key)) = (keygen_id, maybe_local_key) {
				let t = threshold;

				start_rx
					.await
					.map_err(|err| DKGError::StartOffline { reason: err.to_string() })?;

				params.handle.set_status(MetaHandlerStatus::OfflineAndVoting);
				let count_in_batch = unsigned_proposals.len();
				let batch_key = params.get_next_batch_key(&unsigned_proposals);

				log::debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

				if let Some(offline_i) = Self::get_offline_stage_index(&signing_set, keygen_id) {
					log::info!(target: "dkg", "Offline stage index: {}", offline_i);

					// create one offline stage for each unsigned proposal
					let futures = FuturesUnordered::new();
					for unsigned_proposal in unsigned_proposals {
						futures.push(Box::pin(GenericAsyncHandler::new_offline(
							params.clone(),
							unsigned_proposal,
							offline_i,
							signing_set.clone(),
							local_key.clone(),
							t,
							batch_key,
							async_index,
						)?));
					}

					// NOTE: this will block at each batch of unsigned proposals.
					// TODO: Consider not blocking here and allowing processing of
					// each batch of unsigned proposals concurrently
					futures.try_collect::<()>().await.map(|_| ())?;
					log::info!(
						target: "dkg",
						"Concluded all Offline->Voting stages ({} total) for this batch for this node",
						count_in_batch
					);
				} else {
					log::warn!(target: "dkg", "🕸️  We are not among signers, skipping");
					return Err(DKGError::GenericError {
						reason: "We are not among signers, skipping".to_string(),
					})
				}
			} else {
				log::info!(target: "dkg", "Will skip keygen since local is NOT in best authority set");
			}

			Ok(())
		}
		.then(|res| async move {
			status_handle.set_status(MetaHandlerStatus::Complete);
			// print the res value.
			log::info!(target: "dkg", "🕸️  Signing protocol concluded with {:?}", res);
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					log::info!(target: "dkg", "Stopper has been called {:?}", res1);
					Err(DKGError::GenericError {
						reason: format!("Stopper has been called {:?}", res1)
					})
				}
			}
		});

		Ok(GenericAsyncHandler { protocol })
	}

	#[allow(clippy::too_many_arguments)]
	fn new_offline<BI: BlockchainInterface + 'a>(
		params: AsyncProtocolParameters<BI>,
		unsigned_proposal: UnsignedProposal,
		i: u16,
		s_l: Vec<u16>,
		local_key: LocalKey<Secp256k1>,
		threshold: u16,
		batch_key: BatchKey,
		async_index: u8,
	) -> Result<GenericAsyncHandler<'a, <OfflineStage as StateMachineHandler>::Return>, DKGError> {
		let channel_type = ProtocolType::Offline {
			unsigned_proposal: Arc::new(unsigned_proposal.clone()),
			i,
			s_l: s_l.clone(),
			local_key: Arc::new(local_key.clone()),
		};
		let early_handle = params.handle.broadcaster.subscribe();
		new_inner(
			(unsigned_proposal, i, early_handle, threshold, batch_key),
			OfflineStage::new(i, s_l, local_key)
				.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
			params,
			channel_type,
			async_index,
			DKGMsgStatus::ACTIVE,
		)
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new_voting<BI: BlockchainInterface + 'a>(
		params: AsyncProtocolParameters<BI>,
		completed_offline_stage: CompletedOfflineStage,
		unsigned_proposal: UnsignedProposal,
		party_ind: PartyIndex,
		rx: Receiver<Arc<SignedDKGMessage<Public>>>,
		threshold: Threshold,
		batch_key: BatchKey,
		async_index: u8,
	) -> Result<GenericAsyncHandler<'a, ()>, DKGError> {
		let protocol = Box::pin(async move {
			let ty = ProtocolType::Voting {
				offline_stage: Arc::new(completed_offline_stage.clone()),
				unsigned_proposal: Arc::new(unsigned_proposal.clone()),
				i: party_ind,
			};

			// the below wrapper will map signed messages into unsigned messages
			let incoming = rx;
			let incoming_wrapper = &mut IncomingAsyncProtocolWrapper::new(incoming, ty, &params);
			let (_, session_id, id) = get_party_session_id(&params);
			// the first step is to generate the partial sig based on the offline stage
			let number_of_parties = params.best_authorities.len();

			log::info!(target: "dkg", "Will now begin the voting stage with n={} parties for idx={}", number_of_parties, party_ind);

			let hash_of_proposal = unsigned_proposal.hash().ok_or_else(|| DKGError::Vote {
				reason: "The unsigned proposal for this stage is invalid".to_string(),
			})?;

			let message = BigInt::from_bytes(&hash_of_proposal);
			let offline_stage_pub_key = &completed_offline_stage.public_key().clone();

			let (signing, partial_signature) =
				SignManual::new(message.clone(), completed_offline_stage)
					.map_err(|err| Self::convert_mpc_sign_error_to_dkg_error(err))?;

			let partial_sig_bytes = serde_json::to_vec(&partial_signature).unwrap();

			let payload = DKGMsgPayload::Vote(DKGVoteMessage {
				party_ind,
				// use the hash of proposal as "round key" ONLY for purposes of ensuring
				// uniqueness We only want voting to happen amongst voters under the SAME
				// proposal, not different proposals This is now especially necessary since we
				// are allowing for parallelism now
				round_key: Vec::from(&hash_of_proposal as &[u8]),
				partial_signature: partial_sig_bytes,
				async_index,
			});

			// now, broadcast the data
			let unsigned_dkg_message =
				DKGMessage { id, status: DKGMsgStatus::ACTIVE, payload, session_id };
			params.engine.sign_and_send_msg(unsigned_dkg_message)?;

			// we only need a threshold count of sigs
			let number_of_partial_sigs = threshold as usize;
			let mut sigs = Vec::with_capacity(number_of_partial_sigs);

			log::info!(target: "dkg", "Must obtain {} partial sigs to continue ...", number_of_partial_sigs);

			while let Some(msg) = incoming_wrapper.next().await {
				if let DKGMsgPayload::Vote(dkg_vote_msg) = msg.body.payload {
					// only process messages which are from the respective proposal
					if dkg_vote_msg.round_key.as_slice() == hash_of_proposal {
						log::info!(target: "dkg", "Found matching round key!");
						let partial = serde_json::from_slice::<PartialSignature>(
							&dkg_vote_msg.partial_signature,
						)
						.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
						sigs.push(partial);
						log::info!(target: "dkg", "There are now {} partial sigs ...", sigs.len());
						if sigs.len() == number_of_partial_sigs {
							break
						}
					} else {
						//log::info!(target: "dkg", "Skipping DKG vote message since round
						// keys did not match");
					}
				}
			}

			log::info!("RD0 on {} for {:?}", party_ind, hash_of_proposal);

			if sigs.len() != number_of_partial_sigs {
				log::error!(target: "dkg", "Received number of signs not equal to expected (received: {} | expected: {})", sigs.len(), number_of_partial_sigs);
				return Err(DKGError::Vote {
					reason: "Invalid number of received partial sigs".to_string(),
				})
			}

			log::info!("RD1");
			let signature = signing
				.complete(&sigs)
				.map_err(|err| Self::convert_mpc_sign_error_to_dkg_error(err))?;

			log::info!("RD2");
			verify(&signature, offline_stage_pub_key, &message).map_err(|err| DKGError::Vote {
				reason: format!("Verification of voting stage failed with error : {:?}", err),
			})?;

			log::info!("RD3");
			params.engine.process_vote_result(
				signature,
				unsigned_proposal,
				session_id,
				batch_key,
				message,
			)
		});

		Ok(GenericAsyncHandler { protocol })
	}

	/// Returns our party's index in signers vec if any.
	/// Indexing starts from 1.
	/// OfflineStage must be created using this index if present (not the original keygen index)
	fn get_offline_stage_index(s_l: &[u16], keygen_party_idx: u16) -> Option<u16> {
		(1..)
			.zip(s_l)
			.find(|(_i, keygen_i)| keygen_party_idx == **keygen_i)
			.map(|r| r.0)
	}

	/// Converts the multi_party_ecdsa SignError to DKG error
	/// The aim of the function is to filter out the bad_actors from the mp_ecdsa errors that
	/// contain them
	fn convert_mpc_sign_error_to_dkg_error(
		error: multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::SignError,
	) -> DKGError {
		use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::ProceedError::*;
		match error {
			LocalSigning(Round1(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },
			CompleteSigning(Round1(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },

			LocalSigning(Round2Stage4(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },
			CompleteSigning(Round2Stage4(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },

			LocalSigning(Round3(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },
			CompleteSigning(Round3(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },

			LocalSigning(Round5(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },
			CompleteSigning(Round5(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },

			LocalSigning(Round6VerifyProof(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },
			CompleteSigning(Round6VerifyProof(e)) =>
				DKGError::SignMisbehaviour { reason: e.error_type, bad_actors: e.bad_actors },

			_ => DKGError::SignMisbehaviour { reason: error.to_string(), bad_actors: vec![] },
		}
	}
}
