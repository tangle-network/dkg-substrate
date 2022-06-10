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
use dkg_runtime_primitives::{AuthoritySet, AuthoritySetId, UnsignedProposal};
use futures::{
	channel::mpsc::{UnboundedReceiver, UnboundedSender},
	stream::FuturesUnordered,
	StreamExt, TryStreamExt,
};

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::{Keygen, LocalKey},
	sign::{CompletedOfflineStage, OfflineStage, PartialSignature, SignManual},
};
use parking_lot::RwLock;
use round_based::{async_runtime::watcher::StderrWatcher, AsyncProtocol, Msg, StateMachine};

use serde::Serialize;
use std::{
	fmt::Debug,
	future::Future,
	pin::Pin,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	task::{Context, Poll},
};
use tokio::sync::broadcast::Receiver;

use crate::{utils::find_index, worker::KeystoreExt, DKGKeystore};
use dkg_primitives::{
	types::{
		DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage, DKGVoteMessage,
		RoundId, SignedDKGMessage,
	},
	utils::select_random_set,
};
use dkg_runtime_primitives::crypto::{AuthorityId, Public};
use futures::FutureExt;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::verify;

use crate::{
	meta_async_rounds::{
		blockchain_interface::BlockChainIface,
		incoming::IncomingAsyncProtocolWrapper,
		remote::{MetaAsyncProtocolRemote, MetaHandlerStatus},
		state_machine_interface::StateMachineIface,
		BatchKey, PartyIndex, ProtocolType, Threshold,
	},
	utils::SendFuture,
};
use crate::meta_async_rounds::state_machine_wrapper::StateMachineWrapper;

/// Once created, the MetaDKGProtocolHandler should be .awaited to begin execution
pub struct MetaAsyncProtocolHandler<'a, Out> {
	protocol: Pin<Box<dyn SendFuture<'a, Out>>>,
}

pub struct AsyncProtocolParameters<BCIface: BlockChainIface> {
	pub blockchain_iface: Arc<BCIface>,
	pub keystore: DKGKeystore,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	pub batch_id_gen: Arc<AtomicU64>,
	pub handle: MetaAsyncProtocolRemote<BCIface::Clock>,
	pub round_id: RoundId,
}

#[derive(Debug, Clone, Default)]
pub struct CurrentRoundBlame {
	/// a numbers of messages yet to recieve
	pub unrecieved_messages: u16,
	/// a list of uncorporative parties
	pub blamed_parties: Vec<u16>,
}

impl CurrentRoundBlame {
	pub fn empty() -> Self {
		Self::default()
	}
}

impl<BCIface: BlockChainIface> AsyncProtocolParameters<BCIface> {
	pub fn get_next_batch_key(&self, batch: &[UnsignedProposal]) -> BatchKey {
		BatchKey { id: self.batch_id_gen.fetch_add(1, Ordering::SeqCst), len: batch.len() }
	}
}

// Manual implementation of Clone due to https://stegosaurusdormant.com/understanding-derive-clone/
impl<BCIface: BlockChainIface> Clone for AsyncProtocolParameters<BCIface> {
	fn clone(&self) -> Self {
		Self {
			round_id: self.round_id,
			blockchain_iface: self.blockchain_iface.clone(),
			keystore: self.keystore.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities: self.best_authorities.clone(),
			authority_public_key: self.authority_public_key.clone(),
			batch_id_gen: self.batch_id_gen.clone(),
			handle: self.handle.clone(),
		}
	}
}

impl<'a, Out: Send + Debug + 'a> MetaAsyncProtocolHandler<'a, Out>
where
	(): Extend<Out>,
{
	/// Top-level function used to begin the execution of async protocols
	pub fn setup<B: BlockChainIface + 'a>(
		params: AsyncProtocolParameters<B>,
		threshold: u16,
	) -> Result<MetaAsyncProtocolHandler<'a, ()>, DKGError> {
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
			let params = params;
			let (keygen_id, _b, _c) = Self::get_party_round_id(&params);
			if let Some(keygen_id) = keygen_id {
				log::info!(target: "dkg", "Will execute keygen since local is in best authority set");
				let t = threshold;
				let n = params.best_authorities.len() as u16;
				// wait for the start signal
				start_rx.await.map_err(|err| DKGError::StartKeygen { reason: err.to_string() })?;

				params.handle.set_status(MetaHandlerStatus::Keygen);

				// causal flow: create 1 keygen then, fan-out to unsigned_proposals.len()
				// offline-stage async subroutines those offline-stages will each automatically
				// proceed with their corresponding voting stages in parallel
				let local_key =
					MetaAsyncProtocolHandler::new_keygen(params.clone(), keygen_id, t, n)?.await?;
				log::debug!(target: "dkg", "Keygen stage complete! Now running concurrent offline->voting stages ...");
				params.handle.set_status(MetaHandlerStatus::AwaitingProposals);

				let mut unsigned_proposals_rx =
					params.handle.unsigned_proposals_rx.lock().take().ok_or_else(|| {
						DKGError::CriticalError {
							reason: "unsigned_proposals_rx already taken".to_string(),
						}
					})?;

				while let Some(Some(unsigned_proposals)) = unsigned_proposals_rx.recv().await {
					params.handle.set_status(MetaHandlerStatus::OfflineAndVoting);
					let count_in_batch = unsigned_proposals.len();
					let batch_key = params.get_next_batch_key(&unsigned_proposals);
					let s_l = &Self::generate_signers(&local_key, t, n, &params)?;

					log::debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

					if let Some(offline_i) = Self::get_offline_stage_index(s_l, keygen_id) {
						log::info!("Offline stage index: {}", offline_i);

						if count_in_batch == 0 {
							log::debug!(target: "dkg", "Skipping batch since len = 0");
							continue
						}

						// create one offline stage for each unsigned proposal
						let futures = FuturesUnordered::new();
						for unsigned_proposal in unsigned_proposals {
							futures.push(Box::pin(MetaAsyncProtocolHandler::new_offline(
								params.clone(),
								unsigned_proposal,
								offline_i,
								s_l.clone(),
								local_key.clone(),
								t,
								batch_key
							)?));
						}

						// NOTE: this will block at each batch of unsigned proposals.
						// TODO: Consider not blocking here and allowing processing of
						// each batch of unsigned proposals concurrently
						futures.try_collect::<()>().await.map(|_| ())?;
						log::info!(
								"Concluded all Offline->Voting stages ({} total) for this batch for this node",
								count_in_batch
							);
					} else {
						log::warn!(target: "dkg", "🕸️  We are not among signers, skipping");
						return Ok(())
					}
				}
			} else {
				log::info!(target: "dkg", "Will skip keygen since local is NOT in best
					 authority set");
			}

			Ok(())
		}.then(|res| async move {
			status_handle.set_status(MetaHandlerStatus::Complete);
			log::info!(target: "dkg", "🕸️  MetaAsyncProtocolHandler completed");
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					log::info!(target: "dkg", "Stopper has been called {:?}", res1);
					Ok(())
				}
			}
		});

		Ok(MetaAsyncProtocolHandler { protocol })
	}

	fn new_keygen<B: BlockChainIface + 'a>(
		params: AsyncProtocolParameters<B>,
		i: u16,
		t: u16,
		n: u16,
	) -> Result<MetaAsyncProtocolHandler<'a, <Keygen as StateMachineIface>::Return>, DKGError> {
		let channel_type = ProtocolType::Keygen { i, t, n };
		MetaAsyncProtocolHandler::new_inner(
			(),
			Keygen::new(i, t, n)
				.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
			params,
			channel_type,
		)
	}

	fn new_offline<B: BlockChainIface + 'a>(
		params: AsyncProtocolParameters<B>,
		unsigned_proposal: UnsignedProposal,
		i: u16,
		s_l: Vec<u16>,
		local_key: LocalKey<Secp256k1>,
		threshold: u16,
		batch_key: BatchKey,
	) -> Result<MetaAsyncProtocolHandler<'a, <OfflineStage as StateMachineIface>::Return>, DKGError>
	{
		let channel_type = ProtocolType::Offline {
			unsigned_proposal: Arc::new(unsigned_proposal.clone()),
			i,
			s_l: s_l.clone(),
			local_key: Arc::new(local_key.clone()),
		};
		let early_handle = params.handle.broadcaster.subscribe();
		MetaAsyncProtocolHandler::new_inner(
			(unsigned_proposal, i, early_handle, threshold, batch_key),
			OfflineStage::new(i, s_l, local_key)
				.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
			params,
			channel_type,
		)
	}

	fn new_inner<SM: StateMachineIface + 'static, B: BlockChainIface + 'a>(
		additional_param: SM::AdditionalReturnParam,
		sm: SM,
		params: AsyncProtocolParameters<B>,
		channel_type: ProtocolType,
	) -> Result<MetaAsyncProtocolHandler<'a, SM::Return>, DKGError>
	where
		<SM as StateMachine>::Err: Send + Debug,
		<SM as StateMachine>::MessageBody: Send,
		<SM as StateMachine>::MessageBody: Serialize,
		<SM as StateMachine>::Output: Send,
	{
		let (incoming_tx_proto, incoming_rx_proto) = SM::generate_channel();
		let (outgoing_tx, outgoing_rx) = futures::channel::mpsc::unbounded();

		let sm = StateMachineWrapper::new(sm, params.handle.current_round_blame_tx.clone());

		let mut async_proto = AsyncProtocol::new(
			sm,
			incoming_rx_proto.map(Ok::<_, <SM as StateMachine>::Err>),
			outgoing_tx,
		)
		.set_watcher(StderrWatcher);

		let params_for_end_of_proto = params.clone();

		let async_proto = Box::pin(async move {
			let res = async_proto
				.run()
				.await
				.map_err(|err| DKGError::GenericError { reason: format!("{:?}", err) })?;

			SM::on_finish(res, params_for_end_of_proto, additional_param).await
		});

		// For taking all unsigned messages generated by the AsyncProtocols, signing them,
		// and thereafter sending them outbound
		let outgoing_to_wire = Self::generate_outgoing_to_wire_fn::<SM, B>(
			params.clone(),
			outgoing_rx,
			channel_type.clone(),
		);

		// For taking raw inbound signed messages, mapping them to unsigned messages, then
		// sending to the appropriate AsyncProtocol
		let inbound_signed_message_receiver = Self::generate_inbound_signed_message_receiver_fn::<
			SM,
			B,
		>(params, channel_type.clone(), incoming_tx_proto);

		// Combine all futures into a concurrent select subroutine
		let protocol = async move {
			tokio::select! {
				proto_res = async_proto => {
					log::info!(target: "dkg", "🕸️  Protocol {:?} Ended: {:?}", channel_type, proto_res);
					proto_res
				},

				outgoing_res = outgoing_to_wire => {
					log::error!(target: "dkg", "🕸️  Outbound Sender Ended: {:?}", outgoing_res);
					Err(DKGError::GenericError { reason: "Outbound sender ended".to_string() })
				},

				incoming_res = inbound_signed_message_receiver => {
					log::error!(target: "dkg", "🕸️  Inbound Receiver Ended: {:?}", incoming_res);
					Err(DKGError::GenericError { reason: "Incoming receiver ended".to_string() })
				}
			}
		};

		Ok(MetaAsyncProtocolHandler { protocol: Box::pin(protocol) })
	}

	pub(crate) fn new_voting<B: BlockChainIface + 'a>(
		params: AsyncProtocolParameters<B>,
		completed_offline_stage: CompletedOfflineStage,
		unsigned_proposal: UnsignedProposal,
		party_ind: PartyIndex,
		rx: Receiver<Arc<SignedDKGMessage<Public>>>,
		threshold: Threshold,
		batch_key: BatchKey,
	) -> Result<MetaAsyncProtocolHandler<'a, ()>, DKGError> {
		let protocol = Box::pin(async move {
			let ty = ProtocolType::Voting {
				offline_stage: Arc::new(completed_offline_stage.clone()),
				unsigned_proposal: Arc::new(unsigned_proposal.clone()),
				i: party_ind,
			};

			// the below wrapper will map signed messages into unsigned messages
			let incoming = rx;
			let incoming_wrapper = &mut IncomingAsyncProtocolWrapper::new(incoming, ty, &params);
			let (_, round_id, id) = Self::get_party_round_id(&params);
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
					.map_err(|err| DKGError::Vote { reason: err.to_string() })?;

			let partial_sig_bytes = serde_json::to_vec(&partial_signature).unwrap();

			let payload = DKGMsgPayload::Vote(DKGVoteMessage {
				party_ind,
				// use the hash of proposal as "round key" ONLY for purposes of ensuring
				// uniqueness We only want voting to happen amongst voters under the SAME
				// proposal, not different proposals This is now especially necessary since we
				// are allowing for parallelism now
				round_key: Vec::from(&hash_of_proposal as &[u8]),
				partial_signature: partial_sig_bytes,
			});

			// now, broadcast the data
			let unsigned_dkg_message = DKGMessage { id, payload, round_id };
			params.blockchain_iface.sign_and_send_msg(unsigned_dkg_message)?;

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
				.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;

			log::info!("RD2");
			verify(&signature, offline_stage_pub_key, &message).map_err(|_err| DKGError::Vote {
				reason: "Verification of voting stage failed".to_string(),
			})?;
			log::info!("RD3");

			params.blockchain_iface.process_vote_result(
				signature,
				unsigned_proposal,
				round_id,
				batch_key,
				message,
			)
		});

		Ok(MetaAsyncProtocolHandler { protocol })
	}

	pub(crate) fn get_party_round_id<B: BlockChainIface + 'a>(
		params: &AsyncProtocolParameters<B>,
	) -> (Option<u16>, AuthoritySetId, Public) {
		let party_ind =
			find_index::<AuthorityId>(&params.best_authorities, &params.authority_public_key)
				.map(|r| r as u16 + 1);
		//let round_id = params.current_validator_set.read().clone().id;
		let round_id = params.round_id;
		let id = params.get_authority_public_key();

		(party_ind, round_id, id)
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

	/// After keygen, this should be called to generate a random set of signers
	/// NOTE: since the random set is called using a symmetric seed to and RNG,
	/// the resulting set is symmetric
	fn generate_signers<B: BlockChainIface + 'a>(
		local_key: &LocalKey<Secp256k1>,
		t: u16,
		n: u16,
		params: &AsyncProtocolParameters<B>,
	) -> Result<Vec<u16>, DKGError> {
		// Select the random subset using the local key as a seed
		let seed = &local_key.public_key().to_bytes(true)[1..];
		let mut final_set = params.blockchain_iface.get_unjailed_signers()?;
		// Mutate the final set if we don't have enough unjailed signers
		if final_set.len() <= t as usize {
			let jailed_set = params.blockchain_iface.get_jailed_signers()?;
			let diff = t as usize + 1 - final_set.len();
			final_set = final_set
				.iter()
				.chain(jailed_set.iter().take(diff))
				.cloned()
				.collect::<Vec<u16>>();
		}

		select_random_set(seed, final_set, t + 1).map(|set| {
			log::info!(target: "dkg", "🕸️  Round Id {:?} | {}-out-of-{} signers: ({:?})", params.round_id, t, n, set);
			set
		}).map_err(|err| {
			DKGError::CreateOfflineStage {
				reason: format!("generate_signers failed, reason: {}", err)
			}
		})
	}

	fn generate_outgoing_to_wire_fn<SM: StateMachineIface + 'a, B: BlockChainIface + 'a>(
		params: AsyncProtocolParameters<B>,
		mut outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>,
		proto_ty: ProtocolType,
	) -> impl SendFuture<'a, ()>
	where
		<SM as StateMachine>::MessageBody: Serialize,
		<SM as StateMachine>::MessageBody: Send,
		<SM as StateMachine>::Output: Send,
	{
		Box::pin(async move {
			// take all unsigned messages, then sign them and send outbound
			//let party_id = proto_ty.get_i();
			while let Some(unsigned_message) = outgoing_rx.next().await {
				log::info!(target: "dkg", "Async proto sent outbound request on node={:?} to: {:?} |(ty: {:?})", unsigned_message.sender, unsigned_message.receiver, &proto_ty);
				let party_id = unsigned_message.sender;
				let serialized_body = serde_json::to_vec(&unsigned_message)
					.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
				let (_, round_id, id) = Self::get_party_round_id(&params);

				let payload =
					match &proto_ty {
						ProtocolType::Keygen { .. } => DKGMsgPayload::Keygen(DKGKeygenMessage {
							round_id: party_id as u64,
							keygen_msg: serialized_body,
						}),
						ProtocolType::Offline { unsigned_proposal, .. } =>
							DKGMsgPayload::Offline(DKGOfflineMessage {
								key: Vec::from(&unsigned_proposal.hash().unwrap() as &[u8]),
								signer_set_id: party_id as u64,
								offline_msg: serialized_body,
							}),
						_ => {
							unreachable!("Should not happen since voting is handled with a custom subroutine")
						},
					};

				let unsigned_dkg_message = DKGMessage { id, payload, round_id };
				params.blockchain_iface.sign_and_send_msg(unsigned_dkg_message)?;
			}

			Err(DKGError::CriticalError {
				reason: "Outbound stream stopped producing items".to_string(),
			})
		})
	}

	fn generate_inbound_signed_message_receiver_fn<
		SM: StateMachineIface + 'a,
		B: BlockChainIface + 'a,
	>(
		params: AsyncProtocolParameters<B>,
		channel_type: ProtocolType,
		to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>,
	) -> impl SendFuture<'a, ()>
	where
		<SM as StateMachine>::MessageBody: Send,
		<SM as StateMachine>::Output: Send,
	{
		Box::pin(async move {
			// the below wrapper will map signed messages into unsigned messages
			let incoming = params.handle.broadcaster.subscribe();
			let mut incoming_wrapper =
				IncomingAsyncProtocolWrapper::new(incoming, channel_type.clone(), &params);

			while let Some(unsigned_message) = incoming_wrapper.next().await {
				if SM::handle_unsigned_message(&to_async_proto, unsigned_message, &channel_type)
					.is_err()
				{
					log::error!(target: "dkg", "Error handling unsigned inbound message. Returning");
					break
				}
			}

			Err::<(), _>(DKGError::CriticalError {
				reason: "Inbound stream stopped producing items".to_string(),
			})
		})
	}
}

impl<Out> Unpin for MetaAsyncProtocolHandler<'_, Out> {}
impl<Out> Future for MetaAsyncProtocolHandler<'_, Out> {
	type Output = Result<Out, DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.protocol.as_mut().poll(cx)
	}
}

#[cfg(test)]
mod tests {
	use crate::{keyring::Keyring, utils::find_index, DKGKeystore};

	use dkg_primitives::{types::DKGError, ProposalNonce};
	use dkg_runtime_primitives::{
		crypto, crypto::AuthorityId, keccak_256, utils::recover_ecdsa_pub_key, AuthoritySet,
		DKGPayloadKey, Proposal, ProposalKind, TypedChainId, UnsignedProposal, KEY_TYPE,
	};
	use futures::{stream::FuturesUnordered, StreamExt, TryStreamExt};
	use itertools::Itertools;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::verify;
	use parking_lot::lock_api::{Mutex, RwLock};
	use rstest::{fixture, rstest};
	use sc_keystore::LocalKeystore;

	use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};

	use std::{collections::HashMap, sync::Arc, time::Duration};

	use crate::meta_async_rounds::{
		blockchain_interface::{BlockChainIface, TestDummyIface},
		meta_handler::{AsyncProtocolParameters, MetaAsyncProtocolHandler},
		remote::MetaAsyncProtocolRemote,
	};
	use tokio_stream::wrappers::IntervalStream;

	// inserts into ks, returns public
	fn generate_new(ks: &dyn SyncCryptoStore, kr: Keyring) -> crypto::Public {
		SyncCryptoStore::ecdsa_generate_new(ks, KEY_TYPE, Some(&kr.to_seed()))
			.unwrap()
			.into()
	}

	#[allow(unused_must_use)]
	fn setup_log() {
		std::env::set_var("RUST_LOG", "trace");
		let _ = env_logger::try_init();
		log::trace!("TRACE enabled");
		log::info!("INFO enabled");
		log::warn!("WARN enabled");
		log::error!("ERROR enabled");
	}

	#[fixture]
	fn raw_keystore() -> SyncCryptoStorePtr {
		Arc::new(LocalKeystore::in_memory())
	}

	#[rstest]
	#[case(2, 3)]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_async_protocol(
		#[case] threshold: u16,
		#[case] num_parties: u16,
		raw_keystore: SyncCryptoStorePtr,
	) -> Result<(), DKGError> {
		setup_log();

		let authority_set = (0..num_parties)
			.into_iter()
			.map(|id| generate_new(&*raw_keystore, Keyring::Custom(id as _)))
			.collect::<Vec<crypto::Public>>();
		assert_eq!(authority_set.len(), authority_set.iter().unique().count()); // assert generated keys are unique

		let dkg_keystore = DKGKeystore::from(Some(raw_keystore));
		let mut validators = AuthoritySet::empty();
		validators.authorities = authority_set.clone();

		let handles = &mut Vec::with_capacity(num_parties as usize);

		let (to_faux_net_tx, mut to_faux_net_rx) = tokio::sync::mpsc::unbounded_channel();

		let async_protocols = FuturesUnordered::new();
		let mut interfaces = vec![];

		for (idx, authority_public_key) in authority_set.iter().enumerate() {
			let party_ind =
				find_index::<AuthorityId>(&authority_set, authority_public_key).unwrap() + 1;
			assert_eq!(party_ind, idx + 1);

			log::info!(target: "dkg", "***Creating Virtual node for id={}***", party_ind);

			let test_iface = TestDummyIface {
				sender: to_faux_net_tx.clone(),
				best_authorities: Arc::new(authority_set.clone()),
				authority_public_key: Arc::new(authority_public_key.clone()),
				vote_results: Arc::new(Mutex::new(HashMap::new())),
				keygen_key: Arc::new(Mutex::new(None)),
			};

			interfaces.push(test_iface.clone());

			// NumberFor::<Block>::from(0u32)
			let handle = MetaAsyncProtocolRemote::new(0u32, 0);

			let async_protocol = create_async_proto_inner(
				test_iface.clone(),
				threshold,
				dkg_keystore.clone(),
				validators.clone(),
				authority_set.clone(),
				authority_public_key.clone(),
				handle.clone(),
			);

			async_protocols.push(async_protocol);
			// start to immediately begin keygen
			handle.start().unwrap();
			handles.push(handle);
		}

		let handles0 = handles.clone();
		// forward messages sent from async protocol to rest of network
		let outbound_to_broadcast_faux_net = async move {
			while let Some(outbound_msg) = to_faux_net_rx.recv().await {
				let outbound_msg = Arc::new(outbound_msg);
				log::info!(target: "dkg", "Forwarding packet from {:?} to signed message receiver", outbound_msg.msg.payload.async_proto_only_get_sender_id().unwrap());
				for handle in handles0.iter() {
					if let Err(err) = handle.deliver_message(outbound_msg.clone()) {
						log::warn!(target: "dkg", "Error delivering message: {}", err.to_string())
					}
				}
			}

			Err::<(), _>(DKGError::CriticalError { reason: "to_faux_net_rx died".to_string() })
		};

		let handles1 = handles.clone();
		let unsigned_proposal_broadcaster = async move {
			let mut ticks =
				IntervalStream::new(tokio::time::interval(Duration::from_millis(1000))).take(1);
			while let Some(_v) = ticks.next().await {
				log::info!(target: "dkg", "Now beginning broadcast of new UnsignedProposals");
				let unsigned_proposals = (0..num_parties)
					.into_iter()
					.map(|idx| UnsignedProposal {
						typed_chain_id: TypedChainId::None,
						key: DKGPayloadKey::RefreshVote(ProposalNonce::from(idx as u32)),
						proposal: Proposal::Unsigned {
							kind: ProposalKind::Refresh,
							data: Vec::from(&(idx as u128).to_be_bytes() as &[u8]),
						},
					})
					.collect::<Vec<UnsignedProposal>>();

				for handle in handles1.iter() {
					handle
						.submit_unsigned_proposals(unsigned_proposals.clone())
						.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?;
				}
			}

			log::info!(target: "dkg", "Done broadcasting UnsignedProposal batches ...");
			for handle in handles.iter() {
				let _ = handle.end_unsigned_proposals();
			}

			Ok(())
		};

		// join these two futures to ensure that when the unsigned proposals broadcaster ends,
		// the entire test doesn't end
		let aux_handler = futures::future::try_join(
			outbound_to_broadcast_faux_net,
			unsigned_proposal_broadcaster,
		);

		let res = tokio::select! {
			res0 = async_protocols.try_collect::<()>() => res0,
			res1 = aux_handler => res1.map(|_| ())
		};

		let _ = res?;

		// Check the signatures
		for iface in interfaces {
			let dkg_public_key = iface.keygen_key.lock().take().unwrap();
			let signed_proposals = iface.vote_results.lock().clone();
			for (_batch, props) in signed_proposals {
				for (prop, sig_recid, message) in props {
					let hash = keccak_256(prop.data());
					assert!(recover_ecdsa_pub_key(&hash, &prop.signature().unwrap()).is_ok());
					assert!(verify(&sig_recid, &dkg_public_key.public_key(), &message).is_ok())
				}
			}
		}

		Ok(())
	}

	fn create_async_proto_inner<'a, B: BlockChainIface + Clone + 'a>(
		b: B,
		threshold: u16,
		keystore: DKGKeystore,
		validator_set: AuthoritySet<crypto::Public>,
		best_authorities: Vec<crypto::Public>,
		authority_public_key: crypto::Public,
		handle: MetaAsyncProtocolRemote<B::Clock>,
	) -> MetaAsyncProtocolHandler<'a, ()> {
		let async_params = AsyncProtocolParameters {
			round_id: 0, // default for unit tests
			blockchain_iface: Arc::new(b),
			keystore,
			current_validator_set: Arc::new(RwLock::new(validator_set)),
			best_authorities: Arc::new(best_authorities),
			authority_public_key: Arc::new(authority_public_key),
			batch_id_gen: Arc::new(Default::default()),
			handle,
		};

		MetaAsyncProtocolHandler::setup(async_params, threshold).unwrap()
	}
}
