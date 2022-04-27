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

use std::{
	fmt::{Debug, Formatter},
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
};

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload, RoundId, SignedDKGMessage};

use futures::stream::Stream;

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey, sign::CompletedOfflineStage,
};
use round_based::Msg;

use crate::{meta_async_protocol::meta_channel::BlockChainIface, worker::AsyncProtocolParameters};
use dkg_runtime_primitives::{crypto::Public, UnsignedProposal};
use tokio_stream::wrappers::BroadcastStream;

pub type SignedMessageBroadcastHandle =
	tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>;

pub struct IncomingAsyncProtocolWrapper<T, B> {
	pub receiver: BroadcastStream<T>,
	round_id: RoundId,
	bc_iface: Arc<B>,
	ty: ProtocolType,
}

impl<T: TransformIncoming, B: BlockChainIface> IncomingAsyncProtocolWrapper<T, B> {
	pub fn new(
		receiver: tokio::sync::broadcast::Receiver<T>,
		ty: ProtocolType,
		params: &AsyncProtocolParameters<B>,
	) -> Self {
		Self {
			receiver: BroadcastStream::new(receiver),
			round_id: params.round_id,
			bc_iface: params.blockchain_iface.clone(),
			ty,
		}
	}
}

#[derive(Clone)]
pub enum ProtocolType {
	Keygen {
		i: u16,
		t: u16,
		n: u16,
	},
	Offline {
		unsigned_proposal: UnsignedProposal,
		i: u16,
		s_l: Vec<u16>,
		local_key: LocalKey<Secp256k1>,
	},
	Voting {
		offline_stage: CompletedOfflineStage,
		unsigned_proposal: UnsignedProposal,
		i: u16,
	},
}

impl ProtocolType {
	pub fn get_i(&self) -> PartyIndex {
		match self {
			Self::Keygen { i, .. } | Self::Offline { i, .. } | Self::Voting { i, .. } => *i,
		}
	}

	pub fn get_unsigned_proposal(&self) -> Option<&UnsignedProposal> {
		match self {
			Self::Offline { unsigned_proposal, .. } | Self::Voting { unsigned_proposal, .. } =>
				Some(unsigned_proposal),
			_ => None,
		}
	}
}

impl Debug for ProtocolType {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			ProtocolType::Keygen { i, t, n } => {
				write!(f, "Keygen: (i, t, n) = ({}, {}, {})", i, t, n)
			},
			ProtocolType::Offline { i, unsigned_proposal, .. } => {
				write!(f, "Offline: (i, proposal) = ({}, {:?})", i, &unsigned_proposal.proposal)
			},
			ProtocolType::Voting { unsigned_proposal, .. } => {
				write!(f, "Voting: proposal = {:?}", &unsigned_proposal.proposal)
			},
		}
	}
}

pub type PartyIndex = u16;
pub type Threshold = u16;
pub type BatchId = u64;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct BatchKey {
	pub len: usize,
	pub id: BatchId,
}

pub trait TransformIncoming: Clone + Send + 'static {
	type IncomingMapped;
	fn transform<B: BlockChainIface>(
		self,
		verify: &B,
		stream_type: &ProtocolType,
		this_round_id: RoundId,
	) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError>
	where
		Self: Sized;
}

impl TransformIncoming for Arc<SignedDKGMessage<Public>> {
	type IncomingMapped = DKGMessage<Public>;
	fn transform<B: BlockChainIface>(
		self,
		verify: &B,
		stream_type: &ProtocolType,
		this_round_id: RoundId,
	) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError>
	where
		Self: Sized,
	{
		match (stream_type, &self.msg.payload) {
			(ProtocolType::Keygen { .. }, DKGMsgPayload::Keygen(..)) |
			(ProtocolType::Offline { .. }, DKGMsgPayload::Offline(..)) |
			(ProtocolType::Voting { .. }, DKGMsgPayload::Vote(..)) => {
				// only clone if the downstream receiver expects this type
				let sender = self.msg.payload.async_proto_only_get_sender_id().unwrap();
				if sender != stream_type.get_i() {
					if self.msg.round_id == this_round_id {
						verify
							.verify_signature_against_authorities(self)
							.map(|body| Some(Msg { sender, receiver: None, body }))
					} else {
						Ok(None)
					}
				} else {
					//log::info!(target: "dkg", "Will skip passing message to state machine since
					// loopback (loopback_id={})", sender);
					Ok(None)
				}
			},

			(_l, _r) => {
				// TODO: route
				//log::warn!("Received message for mixed stage: Local: {:?}, payload: {:?}", l, r);
				Ok(None)
			},
		}
	}
}

impl<T, B> Stream for IncomingAsyncProtocolWrapper<T, B>
where
	T: TransformIncoming,
	B: BlockChainIface,
{
	type Item = Msg<T::IncomingMapped>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let Self { receiver, ty, bc_iface, round_id } = &mut *self;
		let mut receiver = Pin::new(receiver);

		loop {
			match futures::ready!(receiver.as_mut().poll_next(cx)) {
				Some(Ok(msg)) => match msg.transform(&**bc_iface, &*ty, *round_id) {
					Ok(Some(msg)) => return Poll::Ready(Some(msg)),

					Ok(None) => continue,

					Err(err) => {
						log::warn!(target: "dkg", "While mapping signed message, received an error: {:?}", err);
						continue
					},
				},
				Some(Err(err)) => {
					log::error!(target: "dkg", "Stream RECV error: {:?}", err);
					continue
				},
				None => return Poll::Ready(None),
			}
		}
	}
}

pub mod meta_channel {
	use async_trait::async_trait;
	use atomic::Atomic;
	use codec::Encode;
	use curv::{arithmetic::Converter, elliptic::curves::Secp256k1, BigInt};
	use dkg_runtime_primitives::{
		AggregatedPublicKeys, AuthoritySet, AuthoritySetId, DKGApi, Proposal, ProposalKind,
		UnsignedProposal, KEYGEN_TIMEOUT,
	};
	use futures::{
		channel::mpsc::{UnboundedReceiver, UnboundedSender},
		stream::FuturesUnordered,
		StreamExt, TryStreamExt,
	};
	use log::debug;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
		party_i::SignatureRecid,
		state_machine::{
			keygen::{Keygen, LocalKey, ProtocolMessage},
			sign::{
				CompletedOfflineStage, OfflineProtocolMessage, OfflineStage, PartialSignature,
				SignManual,
			},
		},
	};
	use parking_lot::{Mutex, RwLock};
	use round_based::{
		async_runtime::watcher::StderrWatcher, containers::StoreErr, AsyncProtocol, Msg,
		StateMachine,
	};
	use sc_client_api::Backend;
	use sc_network_gossip::GossipEngine;
	use serde::Serialize;
	use sp_runtime::traits::{Block, Header, NumberFor};
	use std::{
		collections::HashMap,
		fmt::Debug,
		future::Future,
		marker::PhantomData,
		pin::Pin,
		sync::{atomic::Ordering, Arc},
		task::{Context, Poll},
	};
	use std::path::PathBuf;
	use tokio::sync::{broadcast::Receiver, mpsc::error::SendError};

	use crate::{
		messages::{dkg_message::sign_and_send_messages, public_key_gossip::gossip_public_key},
		meta_async_protocol::{
			BatchKey, IncomingAsyncProtocolWrapper, PartyIndex, ProtocolType, Threshold,
		},
		proposal::{get_signed_proposal, make_signed_proposal},
		storage::proposals::save_signed_proposals_in_storage,
		utils::find_index,
		worker::{AsyncProtocolParameters, DKGWorker, KeystoreExt},
		Client, DKGKeystore,
	};
	use dkg_primitives::{
		rounds::sign::convert_signature,
		types::{
			DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage,
			DKGPublicKeyMessage, DKGSignedPayload, DKGVoteMessage, RoundId, SignedDKGMessage,
		},
		utils::select_random_set,
	};
	use dkg_runtime_primitives::crypto::{AuthorityId, Public};
	use futures::FutureExt;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::verify;
	use sc_keystore::LocalKeystore;
	use sp_arithmetic::traits::AtLeast32BitUnsigned;
	use sp_runtime::generic::BlockId;
	use crate::persistence::store_localkey;

	pub trait SendFuture<'a, Out: 'a>: Future<Output = Result<Out, DKGError>> + Send + 'a {}
	impl<'a, T, Out: Debug + Send + 'a> SendFuture<'a, Out> for T where
		T: Future<Output = Result<Out, DKGError>> + Send + 'a
	{
	}

	/// Once created, the MetaDKGMessageHandler should be .awaited to begin execution
	pub struct MetaAsyncProtocolHandler<'a, Out> {
		protocol: Pin<Box<dyn SendFuture<'a, Out>>>,
	}

	#[async_trait]
	trait StateMachineIface: StateMachine + Send
	where
		<Self as StateMachine>::Output: Send,
	{
		type AdditionalReturnParam: Debug + Send;
		type Return: Debug + Send;

		fn generate_channel() -> (
			futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>,
			futures::channel::mpsc::UnboundedReceiver<Msg<<Self as StateMachine>::MessageBody>>,
		) {
			futures::channel::mpsc::unbounded()
		}

		fn handle_unsigned_message(
			to_async_proto: &futures::channel::mpsc::UnboundedSender<
				Msg<<Self as StateMachine>::MessageBody>,
			>,
			msg: Msg<DKGMessage<Public>>,
			local_ty: &ProtocolType,
		) -> Result<(), <Self as StateMachine>::Err>;

		async fn on_finish<B: BlockChainIface>(
			_result: <Self as StateMachine>::Output,
			_params: AsyncProtocolParameters<B>,
			_additional_param: Self::AdditionalReturnParam,
		) -> Result<Self::Return, DKGError>;
	}

	#[async_trait]
	impl StateMachineIface for Keygen {
		type AdditionalReturnParam = ();
		type Return = <Self as StateMachine>::Output;
		fn handle_unsigned_message(
			to_async_proto: &UnboundedSender<Msg<ProtocolMessage>>,
			msg: Msg<DKGMessage<Public>>,
			local_ty: &ProtocolType,
		) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { payload, .. } = msg.body;
			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Keygen(msg) => {
					log::info!(target: "dkg", "Handling Keygen inbound message from id={}", msg.round_id);
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Error as Error;
					let message: Msg<ProtocolMessage> =
						serde_json::from_slice(msg.keygen_msg.as_slice())
							.map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;

					if let Some(recv) = message.receiver.as_ref() {
						if *recv != local_ty.get_i() {
							//log::info!("Skipping passing of message to async proto since not
							// intended for local");
							return Ok(())
						}
					}
					to_async_proto
						.unbounded_send(message)
						.map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err),
			}

			Ok(())
		}

		async fn on_finish<BCIface: BlockChainIface>(
			local_key: <Self as StateMachine>::Output,
			params: AsyncProtocolParameters<BCIface>,
			_: Self::AdditionalReturnParam,
		) -> Result<<Self as StateMachine>::Output, DKGError> {
			log::info!(target: "dkg", "Completed keygen stage successfully!");
			// PublicKeyGossip (we need meta handler to handle this)
			// when keygen finishes, we gossip the signed key to peers.
			// [1] create the message, call the "public key gossip" in
			// public_key_gossip.rs:gossip_public_key [2] store public key locally (public_keys.rs:
			// store_aggregated_public_keys)
			let round_id = MetaAsyncProtocolHandler::<()>::get_party_round_id(&params).1;
			let pub_key_msg = DKGPublicKeyMessage {
				round_id,
				pub_key: local_key.public_key().to_bytes(true).to_vec(),
				signature: vec![],
			};

			params.blockchain_iface.gossip_public_key(pub_key_msg)?;
			params.blockchain_iface.store_public_key(local_key.clone(), round_id)?;

			Ok(local_key)
		}
	}

	#[async_trait]
	impl StateMachineIface for OfflineStage {
		type AdditionalReturnParam = (
			UnsignedProposal,
			PartyIndex,
			Receiver<Arc<SignedDKGMessage<Public>>>,
			Threshold,
			BatchKey,
		);
		type Return = ();

		fn handle_unsigned_message(
			to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>,
			msg: Msg<DKGMessage<Public>>,
			local_ty: &ProtocolType,
		) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { payload, .. } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Offline(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::Error as Error;
					let message: Msg<OfflineProtocolMessage> =
						serde_json::from_slice(msg.offline_msg.as_slice())
							.map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;
					if let Some(recv) = message.receiver.as_ref() {
						if *recv != local_ty.get_i() {
							//log::info!("Skipping passing of message to async proto since not
							// intended for local");
							return Ok(())
						}
					}

					if &local_ty.get_unsigned_proposal().unwrap().hash().unwrap() !=
						msg.key.as_slice()
					{
						//log::info!("Skipping passing of message to async proto since not correct
						// unsigned proposal");
						return Ok(())
					}

					to_async_proto
						.unbounded_send(message)
						.map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err),
			}

			Ok(())
		}

		async fn on_finish<BCIface: BlockChainIface>(
			offline_stage: <Self as StateMachine>::Output,
			params: AsyncProtocolParameters<BCIface>,
			unsigned_proposal: Self::AdditionalReturnParam,
		) -> Result<(), DKGError> {
			log::info!(target: "dkg", "Completed offline stage successfully!");
			// take the completed offline stage, and, immediately execute the corresponding voting
			// stage (this will allow parallelism between offline stages executing across the
			// network) NOTE: we pass the generated offline stage id for the i in voting to keep
			// consistency
			MetaAsyncProtocolHandler::new_voting(
				params,
				offline_stage,
				unsigned_proposal.0,
				unsigned_proposal.1,
				unsigned_proposal.2,
				unsigned_proposal.3,
				unsigned_proposal.4,
			)?
			.await?;
			Ok(())
		}
	}

	pub trait BlockChainIface: Send + Sync {
		type Clock: AtLeast32BitUnsigned + Copy + Send + Sync;
		fn verify_signature_against_authorities(
			&self,
			message: Arc<SignedDKGMessage<Public>>,
		) -> Result<DKGMessage<Public>, DKGError>;
		fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError>;
		fn process_vote_result(
			&self,
			signature: SignatureRecid,
			unsigned_proposal: UnsignedProposal,
			round_id: RoundId,
			batch_key: BatchKey,
			message: BigInt,
		) -> Result<(), DKGError>;
		fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError>;
		fn debug_only_stop_after_first_batch(&self) -> bool {
			false
		}
		fn store_public_key(
			&self,
			key: LocalKey<Secp256k1>,
			round_id: RoundId,
		) -> Result<(), DKGError>;
		fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError>;
		fn get_authority_set(&self) -> &Vec<Public>;
		/// Get the unjailed signers
		fn get_unjailed_signers(&self) -> Result<Vec<u16>, DKGError> {
			let jailed_signers = self.get_jailed_signers_inner()?;
			Ok(self
				.get_authority_set()
				.iter()
				.enumerate()
				.filter(|(_, key)| !jailed_signers.contains(key))
				.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
				.collect())
		}

		/// Get the jailed signers
		fn get_jailed_signers(&self) -> Result<Vec<u16>, DKGError> {
			let jailed_signers = self.get_jailed_signers_inner()?;
			Ok(self
				.get_authority_set()
				.iter()
				.enumerate()
				.filter(|(_, key)| jailed_signers.contains(key))
				.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
				.collect())
		}
	}

	#[derive(Clone)]
	pub struct MetaAsyncProtocolRemote<C> {
		status: Arc<Atomic<MetaHandlerStatus>>,
		unsigned_proposals_tx: tokio::sync::mpsc::UnboundedSender<Vec<UnsignedProposal>>,
		unsigned_proposals_rx:
			Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Vec<UnsignedProposal>>>>>,
		stop_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>>,
		stop_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
		started_at: C,
		pub(crate) round_id: RoundId,
	}

	#[derive(Copy, Clone, Debug, Eq, PartialEq)]
	pub enum MetaHandlerStatus {
		Beginning,
		Keygen,
		AwaitingProposals,
		OfflineAndVoting,
		Complete,
	}

	impl<C: AtLeast32BitUnsigned + Copy> MetaAsyncProtocolRemote<C> {
		/// Create at the beginning of each meta handler instantiation
		pub fn new(at: C, round_id: RoundId) -> Self {
			let (unsigned_proposals_tx, unsigned_proposals_rx) =
				tokio::sync::mpsc::unbounded_channel();
			let (stop_tx, stop_rx) = tokio::sync::mpsc::unbounded_channel();

			Self {
				status: Arc::new(Atomic::new(MetaHandlerStatus::Beginning)),
				unsigned_proposals_tx,
				unsigned_proposals_rx: Arc::new(Mutex::new(Some(unsigned_proposals_rx))),
				started_at: at,
				stop_tx: Arc::new(Mutex::new(Some(stop_tx))),
				stop_rx: Arc::new(Mutex::new(Some(stop_rx))),
				round_id,
			}
		}

		pub fn keygen_has_stalled(&self, now: C) -> bool {
			self.get_status() == MetaHandlerStatus::Keygen &&
				(now - self.started_at > KEYGEN_TIMEOUT.into())
		}
	}

	impl<C> MetaAsyncProtocolRemote<C> {
		pub fn get_status(&self) -> MetaHandlerStatus {
			self.status.load(Ordering::SeqCst)
		}

		pub fn set_status(&self, status: MetaHandlerStatus) {
			self.status.store(status, Ordering::SeqCst)
		}

		pub fn is_active(&self) -> bool {
			let status = self.get_status();
			status != MetaHandlerStatus::Beginning && status != MetaHandlerStatus::Complete
		}

		pub fn submit_unsigned_proposals(
			&self,
			unsigned_proposals: Vec<UnsignedProposal>,
		) -> Result<(), SendError<Vec<UnsignedProposal>>> {
			self.unsigned_proposals_tx.send(unsigned_proposals)
		}

		/// Stops the execution of the meta handler, including all internal asynchronous subroutines
		pub fn shutdown(&self) -> Result<(), DKGError> {
			let tx = self.stop_tx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "Shutdown has already been called".to_string(),
			})?;
			tx.send(()).map_err(|_| DKGError::GenericError {
				reason: "Unable to send shutdown signal (already shut down?)".to_string(),
			})
		}

		pub fn is_keygen_finished(&self) -> bool {
			let state = self.get_status();
			match state {
				MetaHandlerStatus::AwaitingProposals |
				MetaHandlerStatus::OfflineAndVoting |
				MetaHandlerStatus::Complete => true,
				_ => false,
			}
		}
	}

	impl<C> Drop for MetaAsyncProtocolRemote<C> {
		fn drop(&mut self) {
			if Arc::strong_count(&self.status) == 2 {
				// at this point, the only instances of this arc are this one, and,
				// presumably the one in the DKG worker. This one is asserted to be the one
				// belonging to the async proto. Signal as complete to allow the DKG worker to move
				// forward
				log::info!(target: "dkg", "[drop code] MetaAsyncProtocol is ending");
				self.set_status(MetaHandlerStatus::Complete);
			}
		}
	}

	pub struct DKGIface<B: Block, BE, C> {
		pub backend: Arc<BE>,
		pub latest_header: Arc<RwLock<Option<B::Header>>>,
		pub client: Arc<C>,
		pub keystore: DKGKeystore,
		pub gossip_engine: Arc<Mutex<GossipEngine<B>>>,
		pub aggregated_public_keys: Arc<Mutex<HashMap<RoundId, AggregatedPublicKeys>>>,
		pub best_authorities: Arc<Vec<Public>>,
		pub authority_public_key: Arc<Public>,
		pub vote_results: Arc<Mutex<HashMap<BatchKey, Vec<Proposal>>>>,
		pub is_genesis: bool,
		pub _pd: PhantomData<BE>,
		pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
		pub local_keystore: Option<Arc<LocalKeystore>>,
		pub local_key_path: Option<PathBuf>
	}

	impl<B, BE, C> BlockChainIface for DKGIface<B, BE, C>
	where
		B: Block,
		BE: Backend<B> + 'static,
		C: Client<B, BE> + 'static,
		C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
	{
		type Clock = NumberFor<B>;

		fn verify_signature_against_authorities(
			&self,
			msg: Arc<SignedDKGMessage<Public>>,
		) -> Result<DKGMessage<Public>, DKGError> {
			let client = &self.client;

			DKGWorker::verify_signature_against_authorities_inner(
				(&*msg).clone(),
				&self.latest_header,
				client,
			)
		}

		fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
			sign_and_send_messages(&self.gossip_engine, &self.keystore, unsigned_msg);
			Ok(())
		}

		fn process_vote_result(
			&self,
			signature: SignatureRecid,
			unsigned_proposal: UnsignedProposal,
			round_id: RoundId,
			batch_key: BatchKey,
			_message: BigInt,
		) -> Result<(), DKGError> {
			// Call worker.rs: handle_finished_round -> Proposal
			// aggregate Proposal into Vec<Proposal>
			let payload_key = unsigned_proposal.key;
			let signature = convert_signature(&signature).ok_or_else(|| {
				DKGError::CriticalError { reason: "Unable to serialize signature".to_string() }
			})?;

			let finished_round = DKGSignedPayload {
				key: round_id.encode(),
				payload: vec![], /* the only place in the codebase that uses this is unit
				                  * testing, and, it is "Webb".encode() */
				signature: signature.encode(),
			};

			let mut lock = self.vote_results.lock();
			let proposals_for_this_batch = lock.entry(batch_key).or_default();

			let proposal =
				get_signed_proposal::<B, C, BE>(&self.backend, finished_round, payload_key)
					.ok_or_else(|| DKGError::CriticalError {
						reason: "Unable to map round to proposal (backend missing?)".to_string(),
					})?;
			proposals_for_this_batch.push(proposal);

			if proposals_for_this_batch.len() == batch_key.len {
				log::info!(target: "dkg", "All proposals have resolved for batch {:?}", batch_key);
				let proposals = lock.remove(&batch_key).unwrap(); // safe unwrap since lock is held
				std::mem::drop(lock);
				save_signed_proposals_in_storage::<B, C, BE>(
					&self.get_authority_public_key(),
					&self.current_validator_set,
					&self.latest_header,
					&self.backend,
					proposals,
				);
			} else {
				log::info!(target: "dkg", "{}/{} proposals have resolved for batch {:?}", proposals_for_this_batch.len(), batch_key.len, batch_key);
			}

			Ok(())
		}

		fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError> {
			gossip_public_key::<B, C, BE>(
				&self.keystore,
				&mut *self.gossip_engine.lock(),
				&mut *self.aggregated_public_keys.lock(),
				key,
			);
			Ok(())
		}

		fn store_public_key(
			&self,
			key: LocalKey<Secp256k1>,
			round_id: RoundId,
		) -> Result<(), DKGError> {
			let sr_pub = self.get_sr25519_public_key();
			if let Some(path) = self.local_key_path.clone() {
				store_localkey(key, round_id, Some(path), self.local_keystore.as_ref(), sr_pub)
					.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
			}

			Ok(())
		}

		fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError> {
			let now = self.latest_header.read().clone().ok_or_else(|| DKGError::CriticalError {
				reason: "latest header does not exist!".to_string(),
			})?;
			let at: BlockId<B> = BlockId::hash(now.hash());
			Ok(self
				.client
				.runtime_api()
				.get_signing_jailed(&at, (&*self.best_authorities).clone())
				.unwrap_or_default())
		}

		fn get_authority_set(&self) -> &Vec<Public> {
			&*self.best_authorities
		}
	}

	#[derive(Clone)]
	pub struct TestDummyIface {
		pub sender: tokio::sync::broadcast::Sender<SignedDKGMessage<Public>>,
		pub best_authorities: Arc<Vec<Public>>,
		pub authority_public_key: Arc<Public>,
		// key is party_index, hash of data. Needed especially for local unit tests
		pub vote_results: Arc<Mutex<HashMap<BatchKey, Vec<(Proposal, SignatureRecid, BigInt)>>>>,
		pub keygen_key: Arc<Mutex<Option<LocalKey<Secp256k1>>>>,
	}

	impl BlockChainIface for TestDummyIface {
		type Clock = u32;
		fn verify_signature_against_authorities(
			&self,
			message: Arc<SignedDKGMessage<Public>>,
		) -> Result<DKGMessage<Public>, DKGError> {
			Ok(message.msg.clone())
		}

		fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
			log::info!(
				"Sending message through iface id={}",
				unsigned_msg.payload.async_proto_only_get_sender_id().unwrap()
			);
			let faux_signed_message = SignedDKGMessage { msg: unsigned_msg, signature: None };
			self.sender
				.send(faux_signed_message)
				.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
			Ok(())
		}

		fn process_vote_result(
			&self,
			signature_rec: SignatureRecid,
			unsigned_proposal: UnsignedProposal,
			round_id: RoundId,
			batch_key: BatchKey,
			message: BigInt,
		) -> Result<(), DKGError> {
			let mut lock = self.vote_results.lock();
			let payload_key = unsigned_proposal.key;
			let signature = convert_signature(&signature_rec).ok_or_else(|| {
				DKGError::CriticalError { reason: "Unable to serialize signature".to_string() }
			})?;

			let finished_round = DKGSignedPayload {
				key: round_id.encode(),
				payload: "Webb".encode(),
				signature: signature.encode(),
			};

			let prop = make_signed_proposal(ProposalKind::EVM, finished_round).unwrap();
			lock.entry(batch_key).or_default().push((prop, signature_rec, message));

			Ok(())
		}

		fn gossip_public_key(&self, _key: DKGPublicKeyMessage) -> Result<(), DKGError> {
			// we do not gossip the public key in the test interface
			Ok(())
		}

		fn debug_only_stop_after_first_batch(&self) -> bool {
			true
		}

		fn store_public_key(&self, key: LocalKey<Secp256k1>, _: RoundId) -> Result<(), DKGError> {
			*self.keygen_key.lock() = Some(key);
			Ok(())
		}

		fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError> {
			Ok(vec![])
		}

		fn get_authority_set(&self) -> &Vec<Public> {
			&*self.best_authorities
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
					reason: "execute called twice with the same AsyncProtocol Parameters"
						.to_string(),
				})?;

			let protocol = async move {
				let params = params;
				let (keygen_id, _b, _c) = Self::get_party_round_id(&params);
				if let Some(keygen_id) = keygen_id {
					log::info!(target: "dkg", "Will execute keygen since local is in best authority set");
					let t = threshold;
					let n = params.best_authorities.len() as u16;
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

					while let Some(unsigned_proposals) = unsigned_proposals_rx.recv().await {
						params.handle.set_status(MetaHandlerStatus::OfflineAndVoting);
						let count_in_batch = unsigned_proposals.len();
						let batch_key = params.get_next_batch_key(&unsigned_proposals);
						let s_l = &Self::generate_signers(&local_key, t, n, &params)?;

						log::debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

						if let Some(offline_i) = Self::get_offline_stage_index(s_l, keygen_id) {
							log::info!("Offline stage index: {}", offline_i);
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

							if params.blockchain_iface.debug_only_stop_after_first_batch() {
								return Ok(())
							}
						} else {
							log::info!(target: "dkg", "🕸️  We are not among signers, skipping");
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
		) -> Result<
			MetaAsyncProtocolHandler<'a, <OfflineStage as StateMachineIface>::Return>,
			DKGError,
		> {
			let channel_type = ProtocolType::Offline {
				unsigned_proposal: unsigned_proposal.clone(),
				i,
				s_l: s_l.clone(),
				local_key: local_key.clone(),
			};
			let early_handle = params.signed_message_broadcast_handle.subscribe();
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

		fn new_voting<B: BlockChainIface + 'a>(
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
					offline_stage: completed_offline_stage.clone(),
					unsigned_proposal: unsigned_proposal.clone(),
					i: party_ind,
				};

				// the below wrapper will map signed messages into unsigned messages
				let incoming = rx;
				let incoming_wrapper =
					&mut IncomingAsyncProtocolWrapper::new(incoming, ty, &params);
				let (_, round_id, id) = Self::get_party_round_id(&params);
				// the first step is to generate the partial sig based on the offline stage
				let number_of_parties = params.best_authorities.len();

				log::info!(target: "dkg", "Will now begin the voting stage with n={} parties for idx={}", number_of_parties, party_ind);

				let hash_of_proposal = unsigned_proposal.hash().ok_or_else(|| DKGError::Vote {
					reason: "The unsigned proposal for this stage is invalid".to_string(),
				})?;

				let message = BigInt::from_bytes(&hash_of_proposal);
				let ref offline_stage_pub_key = completed_offline_stage.public_key().clone();

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
					match msg.body.payload {
						DKGMsgPayload::Vote(dkg_vote_msg) => {
							// only process messages which are from the respective proposal
							if dkg_vote_msg.round_key.as_slice() == hash_of_proposal {
								log::info!(target: "dkg", "Found matching round key!");
								let partial = serde_json::from_slice::<PartialSignature>(
									&dkg_vote_msg.partial_signature,
								)
								.map_err(|err| DKGError::GenericError {
									reason: err.to_string(),
								})?;
								sigs.push(partial);
								log::info!(target: "dkg", "There are now {} partial sigs ...", sigs.len());
								if sigs.len() == number_of_partial_sigs {
									break
								}
							} else {
								//log::info!(target: "dkg", "Skipping DKG vote message since round
								// keys did not match");
							}
						},

						_ => {},
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
				verify(&signature, offline_stage_pub_key, &message).map_err(|_err| {
					DKGError::Vote { reason: "Verification of voting stage failed".to_string() }
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

		fn get_party_round_id<B: BlockChainIface + 'a>(
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
		fn get_offline_stage_index(s_l: &Vec<u16>, keygen_party_idx: u16) -> Option<u16> {
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
			}).map_err(|err| DKGError::CreateOfflineStage { reason: err.to_string() })
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

					let payload = match &proto_ty {
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
				let incoming = params.signed_message_broadcast_handle.subscribe();
				let mut incoming_wrapper =
					IncomingAsyncProtocolWrapper::new(incoming, channel_type.clone(), &params);

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					if let Err(_) = SM::handle_unsigned_message(
						&to_async_proto,
						unsigned_message,
						&channel_type,
					) {
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
}

#[cfg(test)]
mod tests {
	use crate::{
		keyring::Keyring,
		meta_async_protocol::{
			meta_channel::{BlockChainIface, MetaAsyncProtocolHandler, TestDummyIface},
			SignedMessageBroadcastHandle,
		},
		utils::find_index,
		worker::AsyncProtocolParameters,
		DKGKeystore,
	};

	use crate::meta_async_protocol::meta_channel::MetaAsyncProtocolRemote;
	use codec::Encode;
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
	use sc_network_test::Block;
	use sp_core::ByteArray;
	use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};
	use sp_runtime::traits::NumberFor;
	use std::{collections::HashMap, sync::Arc, time::Duration};
	use tokio::sync::mpsc::UnboundedReceiver;
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

		let (signed_message_receiver_tx, signed_message_receiver_rx) =
			tokio::sync::broadcast::channel(4096);
		std::mem::drop(signed_message_receiver_rx);

		let mut handles = Vec::with_capacity(num_parties as usize);

		let (to_faux_net_tx, mut to_faux_net_rx) = tokio::sync::broadcast::channel(4096);

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
				signed_message_receiver_tx.clone(),
				validators.clone(),
				authority_set.clone(),
				authority_public_key.clone(),
				handle.clone(),
			);

			async_protocols.push(async_protocol);
			handles.push(handle);
		}

		// forward messages sent from async protocol to rest of network
		let outbound_to_broadcast_faux_net = async move {
			while let Ok(outbound_msg) = to_faux_net_rx.recv().await {
				log::info!(target: "dkg", "Forwarding packet from {:?} to signed message receiver", outbound_msg.msg.payload.async_proto_only_get_sender_id().unwrap());
				let count = signed_message_receiver_tx
					.send(Arc::new(outbound_msg))
					.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?;
				log::info!(target: "dkg", "Forwarded to {} receivers", count);
			}

			Err::<(), _>(DKGError::CriticalError { reason: "to_faux_net_rx died".to_string() })
		};

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

				for handle in handles.iter() {
					handle
						.submit_unsigned_proposals(unsigned_proposals.clone())
						.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?;
				}
			}

			log::info!(target: "dkg", "Done broadcasting UnsignedProposal batches ...");
			// drop to allow the meta handler's receiver loop to end
			std::mem::drop(handles);

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
		signed_message_receiver: SignedMessageBroadcastHandle,
		validator_set: AuthoritySet<crypto::Public>,
		best_authorities: Vec<crypto::Public>,
		authority_public_key: crypto::Public,
		handle: MetaAsyncProtocolRemote<B::Clock>,
	) -> MetaAsyncProtocolHandler<'a, ()> {
		let async_params = AsyncProtocolParameters {
			round_id: 0, // default for unit tests
			blockchain_iface: Arc::new(b),
			keystore,
			signed_message_broadcast_handle: signed_message_receiver,
			current_validator_set: Arc::new(RwLock::new(validator_set)),
			best_authorities: Arc::new(best_authorities),
			authority_public_key: Arc::new(authority_public_key),
			batch_id_gen: Arc::new(Default::default()),
			handle,
		};

		MetaAsyncProtocolHandler::setup(async_params, threshold).unwrap()
	}
}
