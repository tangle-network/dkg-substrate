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

pub mod blockchain_interface;
pub mod incoming;
pub mod keygen;
pub mod remote;
pub mod sign;
pub mod state_machine;
pub mod state_machine_wrapper;

#[cfg(test)]
pub mod test_utils;

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	crypto::{AuthorityId, Public},
	types::{
		DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGMsgStatus, DKGOfflineMessage,
		SessionId,
	},
	AuthoritySet, AuthoritySetId,
};
use dkg_runtime_primitives::UnsignedProposal;
use futures::{
	channel::mpsc::{UnboundedReceiver, UnboundedSender},
	Future, StreamExt,
};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey, sign::CompletedOfflineStage,
};
use parking_lot::RwLock;
use round_based::{
	async_runtime::{self, watcher::StderrWatcher},
	AsyncProtocol, IsCritical, Msg, StateMachine,
};
use serde::Serialize;
use std::{
	fmt::{Debug, Formatter},
	pin::Pin,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	task::{Context, Poll},
};

use self::{
	blockchain_interface::BlockchainInterface, remote::AsyncProtocolRemote,
	state_machine::StateMachineHandler, state_machine_wrapper::StateMachineWrapper,
};
use crate::{
	utils::{find_index, SendFuture},
	worker::KeystoreExt,
	DKGKeystore,
};
use incoming::IncomingAsyncProtocolWrapper;

pub struct AsyncProtocolParameters<BI: BlockchainInterface> {
	pub engine: Arc<BI>,
	pub keystore: DKGKeystore,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	pub batch_id_gen: Arc<AtomicU64>,
	pub handle: AsyncProtocolRemote<BI::Clock>,
	pub session_id: SessionId,
	pub local_key: Option<LocalKey<Secp256k1>>,
	pub db: Arc<dyn crate::db::DKGDbBackend>,
}

impl<BI: BlockchainInterface> Drop for AsyncProtocolParameters<BI> {
	fn drop(&mut self) {
		if self.handle.is_active() && self.handle.is_primary_remote {
			dkg_logging::warn!(
				"AsyncProtocolParameters({})'s handler is still active and now will be dropped!!!",
				self.session_id
			);
		} else if self.handle.is_primary_remote {
			dkg_logging::debug!(
				"AsyncProtocolParameters({})'s handler is going to be dropped",
				self.session_id
			);
		} else {
			dkg_logging::debug!(
				"AsyncProtocolParameters({})'s handler is going to be dropped",
				self.session_id
			);
		}
	}
}

impl<BI: BlockchainInterface> KeystoreExt for AsyncProtocolParameters<BI> {
	fn get_keystore(&self) -> &DKGKeystore {
		&self.keystore
	}
}

impl<BI: BlockchainInterface> AsyncProtocolParameters<BI> {
	pub fn get_next_batch_key(&self, batch: &[UnsignedProposal]) -> BatchKey {
		BatchKey { id: self.batch_id_gen.fetch_add(1, Ordering::SeqCst), len: batch.len() }
	}
}

// Manual implementation of Clone due to https://stegosaurusdormant.com/understanding-derive-clone/
impl<BI: BlockchainInterface> Clone for AsyncProtocolParameters<BI> {
	fn clone(&self) -> Self {
		Self {
			session_id: self.session_id,
			engine: self.engine.clone(),
			keystore: self.keystore.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities: self.best_authorities.clone(),
			authority_public_key: self.authority_public_key.clone(),
			batch_id_gen: self.batch_id_gen.clone(),
			handle: self.handle.clone(),
			local_key: self.local_key.clone(),
			db: self.db.clone(),
		}
	}
}

#[derive(Debug, Clone, Default)]
pub struct CurrentRoundBlame {
	/// a numbers of messages yet to recieve
	pub unreceived_messages: u16,
	/// a list of uncorporative parties
	pub blamed_parties: Vec<u16>,
}

impl CurrentRoundBlame {
	pub fn empty() -> Self {
		Self::default()
	}
}

#[derive(Debug, Copy, Clone)]
pub enum KeygenRound {
	/// Keygen round is active
	ACTIVE,
	/// Keygen round is queued
	QUEUED,
	/// UNKNOWN
	UNKNOWN,
}

#[derive(Clone)]
pub enum ProtocolType {
	Keygen {
		ty: KeygenRound,
		i: u16,
		t: u16,
		n: u16,
	},
	Offline {
		unsigned_proposal: Arc<UnsignedProposal>,
		i: u16,
		s_l: Vec<u16>,
		local_key: Arc<LocalKey<Secp256k1>>,
	},
	Voting {
		offline_stage: Arc<CompletedOfflineStage>,
		unsigned_proposal: Arc<UnsignedProposal>,
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
			ProtocolType::Keygen { ty, i, t, n } => {
				let ty = match ty {
					KeygenRound::ACTIVE => "ACTIVE",
					KeygenRound::QUEUED => "QUEUED",
					KeygenRound::UNKNOWN => "UNKNOWN",
				};
				write!(f, "{ty} | Keygen: (i, t, n) = ({i}, {t}, {n})")
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

pub struct GenericAsyncHandler<'a, Out> {
	pub protocol: Pin<Box<dyn SendFuture<'a, Out>>>,
}

impl<Out> Unpin for GenericAsyncHandler<'_, Out> {}
impl<Out> Future for GenericAsyncHandler<'_, Out> {
	type Output = Result<Out, DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.protocol.as_mut().poll(cx)
	}
}

pub fn new_inner<SM: StateMachineHandler + 'static, BI: BlockchainInterface + 'static>(
	additional_param: SM::AdditionalReturnParam,
	sm: SM,
	params: AsyncProtocolParameters<BI>,
	channel_type: ProtocolType,
	async_index: u8,
	status: DKGMsgStatus,
) -> Result<GenericAsyncHandler<'static, SM::Return>, DKGError>
where
	<SM as StateMachine>::Err: Send + Debug,
	<SM as StateMachine>::MessageBody: Send,
	<SM as StateMachine>::MessageBody: Serialize,
	<SM as StateMachine>::Output: Send,
{
	let (incoming_tx_proto, incoming_rx_proto) = SM::generate_channel();
	let (outgoing_tx, outgoing_rx) = futures::channel::mpsc::unbounded();

	let session_id = params.session_id;
	let sm = StateMachineWrapper::new(
		sm,
		session_id,
		channel_type.clone(),
		params.handle.current_round_blame_tx.clone(),
	);

	let mut async_proto = AsyncProtocol::new(
		sm,
		incoming_rx_proto.map(Ok::<_, <SM as StateMachine>::Err>),
		outgoing_tx,
	)
	.set_watcher(StderrWatcher);

	let params_for_end_of_proto = params.clone();

	let async_proto = Box::pin(async move {
		// Loop and wait for the protocol to finish.
		loop {
			let res = async_proto.run().await;
			match res {
				Ok(v) =>
					return SM::on_finish(v, params_for_end_of_proto, additional_param, async_index)
						.await,
				Err(err) => match err {
					async_runtime::Error::Recv(e) |
					async_runtime::Error::Proceed(e) |
					async_runtime::Error::HandleIncoming(e) |
					async_runtime::Error::HandleIncomingTimeout(e) |
					async_runtime::Error::Finish(e)
						if e.is_critical() =>
					{
						dkg_logging::error!(target: "dkg", "Async Proto Cought Critical Error: {e:?}");
						return Err(DKGError::GenericError { reason: format!("{e:?}") })
					},
					async_runtime::Error::Send(e) => {
						dkg_logging::error!(target: "dkg", "Async Proto Failed to send outgoing messages: {e:?}");
						return Err(DKGError::GenericError { reason: format!("{e:?}") })
					},
					async_runtime::Error::ProceedPanicked(e) => {
						dkg_logging::error!(target: "dkg", "Async Proto `proceed` method panicked: {e:?}");
						return Err(DKGError::GenericError { reason: format!("{e:?}") })
					},
					async_runtime::Error::InternalError(e) => {
						dkg_logging::error!(target: "dkg", "Async Proto Internal Error: {e:?}");
						return Err(DKGError::GenericError { reason: format!("{e:?}") })
					},
					async_runtime::Error::Exhausted => {
						dkg_logging::error!(target: "dkg", "Async Proto Exhausted");
						return Err(DKGError::GenericError { reason: String::from("Exhausted") })
					},
					async_runtime::Error::RecvEof => {
						dkg_logging::error!(target: "dkg", "Async Proto Incoming channel closed");
						return Err(DKGError::GenericError {
							reason: String::from("RecvEof: Incomming channel closed"),
						})
					},
					async_runtime::Error::BadStateMachine(e) => {
						dkg_logging::error!(target: "dkg", "Async Proto Bad State Machine: {e:?}");
						return Err(DKGError::GenericError { reason: format!("{e:?}") })
					},
					_ => {
						// If the protocol errored, but it's not a critical error, then we
						// should continue to run the protocol.
						dkg_logging::error!(target: "dkg", "Async Proto Cought Non-Critical Error: {err:?}");
					},
				},
			};
		}
	});

	// For taking all unsigned messages generated by the AsyncProtocols, signing them,
	// and thereafter sending them outbound
	let outgoing_to_wire = generate_outgoing_to_wire_fn::<SM, BI>(
		params.clone(),
		outgoing_rx,
		channel_type.clone(),
		async_index,
		status,
	);

	// For taking raw inbound signed messages, mapping them to unsigned messages, then
	// sending to the appropriate AsyncProtocol
	let inbound_signed_message_receiver = generate_inbound_signed_message_receiver_fn::<SM, BI>(
		params,
		channel_type.clone(),
		incoming_tx_proto,
	);

	// Spawn the 3 tasks
	// 1. The inbound task (we will abort that task if the protocol finished)
	let handle = tokio::spawn(inbound_signed_message_receiver);
	// 2. The outbound task (will stop if the protocol finished, after flushing the messages to the
	// network.)
	let handle2 = tokio::spawn(outgoing_to_wire);
	// 3. The async protocol itself
	let protocol = async move {
		let res = async_proto.await;
		dkg_logging::info!(target: "dkg", "🕸️  Protocol {:?} Ended: {:?}", channel_type.clone(), res);
		// Abort the inbound task
		handle.abort();
		// Wait for the outbound task to finish
		// TODO: We should probably have a timeout here, and if the outbound task doesn't finish
		// within a reasonable time, we should abort it.
		match handle2.await {
			Ok(Ok(_)) => dkg_logging::info!(target: "dkg", "🕸️  Outbound task finished"),
			Ok(Err(err)) =>
				dkg_logging::error!(target: "dkg", "🕸️  Outbound task errored: {:?}", err),
			Err(_) => dkg_logging::error!(target: "dkg", "🕸️  Outbound task aborted"),
		}
		res
	};
	Ok(GenericAsyncHandler { protocol: Box::pin(protocol) })
}

fn get_party_session_id<'a, BI: BlockchainInterface + 'a>(
	params: &AsyncProtocolParameters<BI>,
) -> (Option<u16>, AuthoritySetId, Public) {
	let party_ind =
		find_index::<AuthorityId>(&params.best_authorities, &params.authority_public_key)
			.map(|r| r as u16 + 1);
	let session_id = params.session_id;
	let id = params.get_authority_public_key();

	(party_ind, session_id, id)
}

fn generate_outgoing_to_wire_fn<
	SM: StateMachineHandler + 'static,
	BI: BlockchainInterface + 'static,
>(
	params: AsyncProtocolParameters<BI>,
	outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>,
	proto_ty: ProtocolType,
	async_index: u8,
	status: DKGMsgStatus,
) -> impl SendFuture<'static, ()>
where
	<SM as StateMachine>::MessageBody: Serialize,
	<SM as StateMachine>::MessageBody: Send,
	<SM as StateMachine>::Output: Send,
{
	Box::pin(async move {
		let mut outgoing_rx = outgoing_rx.fuse();
		// take all unsigned messages, then sign them and send outbound
		loop {
			// Here is a few explanations about the next few lines:
			// We wait for a message to be available on the channel, and then we take it.
			// this returns an Option<Msg>, which is None if the channel is closed.
			// the channel could be closed if the protocol is finished, since the last sender is
			// dropped. hence, we will break the loop and return.
			let unsigned_message = match outgoing_rx.next().await {
				Some(msg) => msg,
				None => {
					dkg_logging::debug!(target: "dkg", "🕸️  Outgoing Receiver Ended");
					break
				},
			};

			let (maybe_party_i, session_id, id) = get_party_session_id(&params);

			// If we ever hit this case, where we can't find our index in the best authority set, we
			// should probably panic, since we are not supposed to be running the protocol if we
			// are not in the best authority set.
			if maybe_party_i.is_none() {
				dkg_logging::error!(
					target: "dkg",
					"BUG: Could not find our index in the best authority set {:?} at session {session_id}",
					params.best_authorities,
				);
				break
			}
			dkg_logging::info!(target: "dkg", "Async proto sent outbound request in session={} from={:?} to={:?} | (ty: {:?})", params.session_id, unsigned_message.sender, unsigned_message.receiver, &proto_ty);
			let party_id = unsigned_message.sender;
			let serialized_body = match serde_json::to_vec(&unsigned_message) {
				Ok(value) => value,
				Err(err) => {
					dkg_logging::error!(target: "dkg", "Failed to serialize message: {:?}, Skipping..", err);
					continue
				},
			};

			let payload = match &proto_ty {
				ProtocolType::Keygen { .. } => DKGMsgPayload::Keygen(DKGKeygenMessage {
					sender_id: party_id,
					keygen_msg: serialized_body,
				}),
				ProtocolType::Offline { unsigned_proposal, .. } =>
					DKGMsgPayload::Offline(DKGOfflineMessage {
						key: Vec::from(&unsigned_proposal.hash().unwrap() as &[u8]),
						signer_set_id: party_id as u64,
						offline_msg: serialized_body,
						async_index,
					}),
				_ => {
					unreachable!(
						"Should not happen since voting is handled with a custom subroutine"
					)
				},
			};

			let unsigned_dkg_message = DKGMessage { sender_id: id, status, payload, session_id };
			if let Err(err) = params.engine.sign_and_send_msg(unsigned_dkg_message) {
				dkg_logging::error!(target: "dkg", "Async proto failed to send outbound message: {:?}", err);
			} else {
				dkg_logging::info!(target: "dkg", "🕸️  Async proto sent outbound message: {:?}", &proto_ty);
			}

			// check the status of the async protocol.
			// if it has completed or terminated then break out of the loop.
			if params.handle.is_completed() || params.handle.is_terminated() {
				dkg_logging::debug!(target: "dkg", "🕸️  Async proto is completed or terminated, breaking out of incoming loop");
				break
			}
		}
		Ok(())
	})
}

pub fn generate_inbound_signed_message_receiver_fn<
	SM: StateMachineHandler + 'static,
	BI: BlockchainInterface + 'static,
>(
	params: AsyncProtocolParameters<BI>,
	channel_type: ProtocolType,
	to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>,
) -> impl SendFuture<'static, ()>
where
	<SM as StateMachine>::MessageBody: Send,
	<SM as StateMachine>::Output: Send,
{
	Box::pin(async move {
		// the below wrapper will map signed messages into unsigned messages
		let incoming = params.handle.broadcaster.subscribe();
		let incoming_wrapper =
			IncomingAsyncProtocolWrapper::new(incoming, channel_type.clone(), &params);
		// we use fuse here, since normally, once a stream has returned `None` from calling
		// `next()` any further calls could exhibit bad behavior such as block forever, panic, never
		// return, etc. that's why we use fuse here to ensure that it has defined semantics,
		// which means, once it returns `None` we will never poll that stream again.
		let mut incoming_wrapper = incoming_wrapper.fuse();
		loop {
			let unsigned_message = match incoming_wrapper.next().await {
				Some(msg) => msg,
				None => {
					dkg_logging::debug!(target: "dkg", "🕸️  Inbound Receiver Ended");
					break
				},
			};

			if SM::handle_unsigned_message(&to_async_proto, unsigned_message, &channel_type)
				.is_err()
			{
				dkg_logging::error!(target: "dkg", "Error handling unsigned inbound message. Returning");
				break
			}

			// check the status of the async protocol.
			if params.handle.is_completed() || params.handle.is_terminated() {
				dkg_logging::debug!(target: "dkg", "🕸️  Async proto is completed or terminated, breaking out of inbound loop");
				break
			}
		}
		Ok(())
	})
}
