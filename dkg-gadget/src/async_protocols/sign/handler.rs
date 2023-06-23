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
use futures::StreamExt;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey,
	sign::{CompletedOfflineStage, OfflineStage, PartialSignature, SignManual},
};

use std::{collections::HashSet, fmt::Debug, sync::Arc, time::Duration};

use crate::async_protocols::{
	blockchain_interface::BlockchainInterface,
	incoming::IncomingAsyncProtocolWrapper,
	new_inner,
	remote::{MetaHandlerStatus, ShutdownReason},
	state_machine::StateMachineHandler,
	AsyncProtocolParameters, BatchKey, GenericAsyncHandler, KeygenPartyId, OfflinePartyId,
	ProtocolType, Threshold,
};
use dkg_logging::debug_logger::RoundsEventType;
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgPayload, DKGVoteMessage, SignedDKGMessage,
};
use dkg_runtime_primitives::{crypto::Public, MaxAuthorities};
use futures::FutureExt;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::verify,
	state_machine::sign::SignError::{CompleteSigning, LocalSigning},
};

impl<Out: Send + Debug + 'static> GenericAsyncHandler<'static, Out>
where
	(): Extend<Out>,
{
	/// Top level function for setting up signing
	pub fn setup_signing<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		threshold: u16,
		unsigned_proposal: UnsignedProposal<<BI as BlockchainInterface>::MaxProposalLength>,
		s_l: Vec<KeygenPartyId>,
	) -> Result<GenericAsyncHandler<'static, ()>, DKGError> {
		assert!(threshold + 1 == s_l.len() as u16, "Signing set must be of size threshold + 1");
		let status_handle = params.handle.clone();
		let mut stop_rx =
			status_handle.stop_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let start_rx =
			status_handle.start_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let logger0 = params.logger.clone();
		let logger2 = params.logger.clone();

		let protocol = async move {
			let maybe_local_key = params.local_key.clone();
			if let Some(local_key) = maybe_local_key {
				let t = threshold;

				start_rx
					.await
					.map_err(|err| DKGError::StartOffline { reason: err.to_string() })?;

				params.handle.set_status(MetaHandlerStatus::OfflineAndVoting);
				let batch_key = params.get_next_batch_key();

				params.logger.debug_signing("Received unsigned proposal");

				if let Ok(offline_i) = params.party_i.try_to_offline_party_id(&s_l) {
					params.logger.info_signing(format!(
						"Party Index converted to offline stage Index : {:?}",
						params.party_i
					));
					params.logger.info_signing(format!("Offline stage index: {offline_i}"));

					GenericAsyncHandler::new_offline(
						params.clone(),
						unsigned_proposal,
						offline_i,
						s_l.clone(),
						local_key.clone(),
						t,
						batch_key,
					)?
					.await?;

					params.logger.info_signing("Concluded Offline->Voting stage for this node");
				} else {
					params.logger.warn_signing("🕸️  We are not among signers, skipping".to_string());
					return Err(DKGError::GenericError {
						reason: "We are not among signers, skipping".to_string(),
					})
				}
			} else {
				return Err(DKGError::GenericError {
					reason: "Will skip signing since local key does not exist".to_string(),
				})
			}

			Ok(())
		}
		.then(|res| async move {
			status_handle.set_status(MetaHandlerStatus::Complete);
			// print the res value.
			logger0.info_signing(format!("🕸️  Signing protocol concluded with {res:?}"));
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					logger2.info_signing(format!("Stopper has been called {res1:?}"));
					if let Some(res1) = res1 {
						if res1 == ShutdownReason::DropCode {
							Ok(())
						} else {
							Err(DKGError::GenericError { reason: "Signing has stalled".into() })
						}
					} else {
						Ok(())
					}
				}
			}
		});

		Ok(GenericAsyncHandler { protocol })
	}

	#[allow(clippy::too_many_arguments)]
	fn new_offline<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		unsigned_proposal: UnsignedProposal<<BI as BlockchainInterface>::MaxProposalLength>,
		offline_i: OfflinePartyId,
		s_l: Vec<KeygenPartyId>,
		local_key: LocalKey<Secp256k1>,
		threshold: u16,
		batch_key: BatchKey,
	) -> Result<
		GenericAsyncHandler<'static, <OfflineStage as StateMachineHandler<BI>>::Return>,
		DKGError,
	> {
		let channel_type = ProtocolType::Offline {
			unsigned_proposal: Arc::new(unsigned_proposal.clone()),
			i: offline_i,
			s_l: s_l.clone(),
			local_key: Arc::new(local_key.clone()),
			associated_block_id: params.associated_block_id,
		};
		let s_l_raw = s_l.into_iter().map(|party_i| *party_i.as_ref()).collect();
		new_inner(
			(unsigned_proposal, offline_i, threshold, batch_key),
			OfflineStage::new(*offline_i.as_ref(), s_l_raw, local_key)
				.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
			params,
			channel_type,
		)
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new_voting<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		completed_offline_stage: CompletedOfflineStage,
		unsigned_proposal: UnsignedProposal<<BI as BlockchainInterface>::MaxProposalLength>,
		offline_i: OfflinePartyId,
		rx: tokio::sync::mpsc::UnboundedReceiver<SignedDKGMessage<Public>>,
		threshold: Threshold,
		batch_key: BatchKey,
	) -> Result<GenericAsyncHandler<'static, ()>, DKGError> {
		let protocol = Box::pin(async move {
			let unsigned_proposal_hash = unsigned_proposal.hash().expect("Should not fail");
			let ty = ProtocolType::Voting {
				offline_stage: Arc::new(completed_offline_stage.clone()),
				unsigned_proposal: Arc::new(unsigned_proposal.clone()),
				i: offline_i,
				associated_block_id: params.associated_block_id,
			};

			params.logger.round_event(
				&ty,
				RoundsEventType::ProceededToRound { session: params.session_id, round: 0 },
			);

			let mut incoming_wrapper =
				IncomingAsyncProtocolWrapper::new(rx, ty.clone(), params.clone());
			// the first step is to generate the partial sig based on the offline stage
			let number_of_parties = params.best_authorities.len();

			params.logger.info_signing(format!(
				"Will now begin the voting stage with n={number_of_parties} parties with offline_i={offline_i}"
			));

			let hash_of_proposal = unsigned_proposal.hash().ok_or_else(|| DKGError::Vote {
				reason: "The unsigned proposal for this stage is invalid".to_string(),
			})?;

			let message = BigInt::from_bytes(&hash_of_proposal);
			let offline_stage_pub_key = &completed_offline_stage.public_key().clone();

			let (signing, partial_signature) =
				SignManual::new(message.clone(), completed_offline_stage)
					.map_err(|err| Self::convert_mpc_sign_error_to_dkg_error_signing(err))?;

			let partial_sig_bytes = serde_json::to_vec(&partial_signature).map_err(|_| {
				DKGError::GenericError { reason: "Partial signature is invalid".to_string() }
			})?;

			let payload = DKGMsgPayload::Vote(DKGVoteMessage {
				party_ind: *offline_i.as_ref(),
				// use the hash of proposal as "round key" ONLY for purposes of ensuring
				// uniqueness We only want voting to happen amongst voters under the SAME
				// proposal, not different proposals This is now especially necessary since we
				// are allowing for parallelism now
				round_key: Vec::from(&hash_of_proposal as &[u8]),
				partial_signature: partial_sig_bytes,
				unsigned_proposal_hash,
			});

			let id = params.authority_public_key.as_ref().clone();
			// now, broadcast the data
			let unsigned_dkg_message = DKGMessage {
				associated_block_id: params.associated_block_id,
				sender_id: id,
				// No recipient for this message, it is broadcasted
				recipient_id: None,
				payload,
				session_id: params.session_id,
			};

			// we have no synchronization mechanism post-offline stage. Sometimes, messages
			// don't get delivered. Thus, we sent multiple messages, and, wait for a while to let
			// other nodes "show up"
			for _ in 0..3 {
				params.engine.sign_and_send_msg(unsigned_dkg_message.clone())?;
				tokio::time::sleep(Duration::from_millis(100)).await;
			}

			// we only need a threshold count of sigs
			let number_of_partial_sigs = threshold as usize;
			let mut sigs = Vec::with_capacity(number_of_partial_sigs);

			params.logger.info_signing(format!(
				"Must obtain {number_of_partial_sigs} partial sigs to continue ..."
			));

			let mut received_sigs = HashSet::new();

			while let Some(msg) = incoming_wrapper.next().await {
				let payload = msg.body.payload.payload().clone();
				params.logger.checkpoint_message_raw(&payload, "CP-Voting-Received");
				if let DKGMsgPayload::Vote(dkg_vote_msg) = msg.body.payload {
					// only process messages which are from the respective proposal
					if dkg_vote_msg.round_key.as_slice() == hash_of_proposal {
						params.logger.checkpoint_message_raw(&payload, "CP-Voting-Received-2");
						if !received_sigs.insert(msg.sender) {
							params.logger.warn_signing(format!(
								"Received duplicate partial sig from {}",
								msg.sender
							));
							params.logger.clear_checkpoint_for_message_raw(&payload);
							continue
						}

						if msg.body.associated_block_id != params.associated_block_id {
							params.logger.warn_signing(format!(
								"Received partial sig from {} with wrong associated block id",
								msg.sender
							));
							params.logger.clear_checkpoint_for_message_raw(&payload);
							continue
						}

						params.logger.checkpoint_message_raw(&payload, "CP-Voting-Received-3");
						params.logger.info_signing("Found matching round key!".to_string());
						let partial = serde_json::from_slice::<PartialSignature>(
							&dkg_vote_msg.partial_signature,
						)
						.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
						params.logger.checkpoint_message_raw(&payload, "CP-Voting-Received-4");
						params.logger.debug(format!(
							"[Sig] Received from {} sig {:?}",
							dkg_vote_msg.party_ind, partial
						));
						sigs.push(partial);
						params
							.logger
							.info_signing(format!("There are now {} partial sigs ...", sigs.len()));
						params.logger.round_event(
							&ty,
							RoundsEventType::ProceededToRound {
								session: params.session_id,
								round: sigs.len(),
							},
						);
						if sigs.len() == number_of_partial_sigs {
							break
						}
					} else {
						params
							.logger
							.info_signing("Found non-matching round key; skipping".to_string());
					}
				}
			}

			params
				.logger
				.info_signing(format!("RD0 on {offline_i} for {hash_of_proposal:?}"));

			if sigs.len() != number_of_partial_sigs {
				params.logger.error_signing(format!(
					"Received number of signs not equal to expected (received: {} | expected: {})",
					sigs.len(),
					number_of_partial_sigs
				));
				return Err(DKGError::Vote {
					reason: "Invalid number of received partial sigs".to_string(),
				})
			}

			params.logger.info_signing("RD1");
			let signature = signing
				.complete(&sigs)
				.map_err(|err| Self::convert_mpc_sign_error_to_dkg_error_signing(err))?;

			params.logger.info_signing("RD2");
			verify(&signature, offline_stage_pub_key, &message).map_err(|err| DKGError::Vote {
				reason: format!("Verification of voting stage failed with error : {err:?}"),
			})?;
			params.logger.info_signing("RD3");
			params.engine.process_vote_result(
				signature,
				unsigned_proposal,
				params.session_id,
				batch_key,
				message,
			)?;
			params.logger.round_event(
				&ty,
				RoundsEventType::ProceededToRound { session: params.session_id, round: 9999999999 },
			);
			Ok(())
		});

		Ok(GenericAsyncHandler { protocol })
	}

	/// Converts the multi_party_ecdsa SignError to DKG error
	/// The aim of the function is to filter out the bad_actors from the mp_ecdsa errors that
	/// contain them
	fn convert_mpc_sign_error_to_dkg_error_signing(
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
