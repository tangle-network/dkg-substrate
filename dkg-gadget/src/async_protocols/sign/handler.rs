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
	blockchain_interface::BlockchainInterface, incoming::IncomingAsyncProtocolWrapper, new_inner,
	remote::MetaHandlerStatus, state_machine::StateMachineHandler, AsyncProtocolParameters,
	BatchKey, GenericAsyncHandler, KeygenPartyId, OfflinePartyId, ProtocolType, Threshold,
};
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgPayload, DKGMsgStatus, DKGVoteMessage, SignedDKGMessage,
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
		unsigned_proposals: Vec<UnsignedProposal<<BI as BlockchainInterface>::MaxProposalLength>>,
		s_l: Vec<KeygenPartyId>,
		async_index: u8,
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

		let protocol = async move {
			let maybe_local_key = params.local_key.clone();
			if let Some(local_key) = maybe_local_key {
				let t = threshold;

				start_rx
					.await
					.map_err(|err| DKGError::StartOffline { reason: err.to_string() })?;

				params.handle.set_status(MetaHandlerStatus::OfflineAndVoting);
				let count_in_batch = unsigned_proposals.len();
				let batch_key = params.get_next_batch_key(&unsigned_proposals);

				dkg_logging::debug!(target: "dkg_gadget", "Got unsigned proposals count {}", unsigned_proposals.len());

				if let Ok(offline_i) = params.party_i.try_to_offline_party_id(&s_l) {
					dkg_logging::info!(target: "dkg", "Offline stage index: {}", offline_i);

					// create one offline stage for each unsigned proposal
					let futures = FuturesUnordered::new();
					for unsigned_proposal in unsigned_proposals {
						futures.push(Box::pin(GenericAsyncHandler::new_offline(
							params.clone(),
							unsigned_proposal,
							offline_i,
							s_l.clone(),
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
					dkg_logging::info!(
						target: "dkg_gadget",
						"Concluded all Offline->Voting stages ({} total) for this batch for this node",
						count_in_batch
					);
				} else {
					dkg_logging::warn!(target: "dkg_gadget", "🕸️  We are not among signers, skipping");
					return Err(DKGError::GenericError {
						reason: "We are not among signers, skipping".to_string(),
					})
				}
			} else {
				dkg_logging::info!(target: "dkg_gadget", "Will skip keygen since local is NOT in best authority set");
			}

			Ok(())
		}
		.then(|res| async move {
			status_handle.set_status(MetaHandlerStatus::Complete);
			// print the res value.
			dkg_logging::info!(target: "dkg_gadget", "🕸️  Signing protocol concluded with {:?}", res);
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					dkg_logging::info!(target: "dkg_gadget", "Stopper has been called {:?}", res1);
					Err(DKGError::GenericError {
						reason: format!("Stopper has been called {res1:?}")
					})
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
		async_index: u8,
	) -> Result<
		GenericAsyncHandler<'static, <OfflineStage as StateMachineHandler<BI>>::Return>,
		DKGError,
	> {
		let channel_type = ProtocolType::Offline {
			unsigned_proposal: Arc::new(unsigned_proposal.clone()),
			i: offline_i,
			s_l: s_l.clone(),
			local_key: Arc::new(local_key.clone()),
		};
		let early_handle = params.handle.broadcaster.subscribe();
		let s_l_raw = s_l.into_iter().map(|party_i| *party_i.as_ref()).collect();
		new_inner(
			(unsigned_proposal, offline_i, early_handle, threshold, batch_key),
			OfflineStage::new(*offline_i.as_ref(), s_l_raw, local_key)
				.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
			params,
			channel_type,
			async_index,
			DKGMsgStatus::ACTIVE,
		)
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new_voting<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		completed_offline_stage: CompletedOfflineStage,
		unsigned_proposal: UnsignedProposal<<BI as BlockchainInterface>::MaxProposalLength>,
		offline_i: OfflinePartyId,
		rx: Receiver<Arc<SignedDKGMessage<Public>>>,
		threshold: Threshold,
		batch_key: BatchKey,
		async_index: u8,
	) -> Result<GenericAsyncHandler<'static, ()>, DKGError> {
		let protocol = Box::pin(async move {
			let ty = ProtocolType::Voting {
				offline_stage: Arc::new(completed_offline_stage.clone()),
				unsigned_proposal: Arc::new(unsigned_proposal.clone()),
				i: offline_i,
			};

			// the below wrapper will map signed messages into unsigned messages
			let incoming = rx;
			let incoming_wrapper = &mut IncomingAsyncProtocolWrapper::new(incoming, ty, &params);
			// the first step is to generate the partial sig based on the offline stage
			let number_of_parties = params.best_authorities.len();

			dkg_logging::info!(target: "dkg_gadget", "Will now begin the voting stage with n={} parties with offline_i={}", number_of_parties, offline_i);

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
				party_ind: *offline_i.as_ref(),
				// use the hash of proposal as "round key" ONLY for purposes of ensuring
				// uniqueness We only want voting to happen amongst voters under the SAME
				// proposal, not different proposals This is now especially necessary since we
				// are allowing for parallelism now
				round_key: Vec::from(&hash_of_proposal as &[u8]),
				partial_signature: partial_sig_bytes,
				async_index,
			});

			let id = params.authority_public_key.as_ref().clone();
			// now, broadcast the data
			let unsigned_dkg_message = DKGMessage {
				sender_id: id,
				// No recipient for this message, it is broadcasted
				recipient_id: None,
				status: DKGMsgStatus::ACTIVE,
				payload,
				session_id: params.session_id,
			};
			params.engine.sign_and_send_msg(unsigned_dkg_message)?;

			// we only need a threshold count of sigs
			let number_of_partial_sigs = threshold as usize;
			let mut sigs = Vec::with_capacity(number_of_partial_sigs);

			dkg_logging::info!(target: "dkg_gadget", "Must obtain {} partial sigs to continue ...", number_of_partial_sigs);

			while let Some(msg) = incoming_wrapper.next().await {
				if let DKGMsgPayload::Vote(dkg_vote_msg) = msg.body.payload {
					// only process messages which are from the respective proposal
					if dkg_vote_msg.round_key.as_slice() == hash_of_proposal {
						dkg_logging::info!(target: "dkg_gadget", "Found matching round key!");
						let partial = serde_json::from_slice::<PartialSignature>(
							&dkg_vote_msg.partial_signature,
						)
						.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
						sigs.push(partial);
						dkg_logging::info!(target: "dkg_gadget", "There are now {} partial sigs ...", sigs.len());
						if sigs.len() == number_of_partial_sigs {
							break
						}
					} else {
						//dkg_logging::info!(target: "dkg_gadget", "Skipping DKG vote message since
						// round keys did not match");
					}
				}
			}

			dkg_logging::info!("RD0 on {} for {:?}", offline_i, hash_of_proposal);

			if sigs.len() != number_of_partial_sigs {
				dkg_logging::error!(target: "dkg_gadget", "Received number of signs not equal to expected (received: {} | expected: {})", sigs.len(), number_of_partial_sigs);
				return Err(DKGError::Vote {
					reason: "Invalid number of received partial sigs".to_string(),
				})
			}

			dkg_logging::info!("RD1");
			let signature = signing
				.complete(&sigs)
				.map_err(|err| Self::convert_mpc_sign_error_to_dkg_error(err))?;

			dkg_logging::info!("RD2");
			verify(&signature, offline_stage_pub_key, &message).map_err(|err| DKGError::Vote {
				reason: format!("Verification of voting stage failed with error : {err:?}"),
			})?;

			dkg_logging::info!("RD3");
			params.engine.process_vote_result(
				signature,
				unsigned_proposal,
				params.session_id,
				batch_key,
				message,
			)
		});

		Ok(GenericAsyncHandler { protocol })
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
