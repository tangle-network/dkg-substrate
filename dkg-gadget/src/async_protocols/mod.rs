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
use sp_runtime::traits::Get;
#[cfg(test)]
pub mod test_utils;

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	crypto::Public,
	types::{
		DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGMsgStatus, DKGOfflineMessage,
		SessionId,
	},
	AuthoritySet,
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
	fmt::{self, Debug, Formatter},
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
use crate::{utils::SendFuture, worker::KeystoreExt, DKGKeystore};
use incoming::IncomingAsyncProtocolWrapper;

pub struct AsyncProtocolParameters<BI: BlockchainInterface> {
	pub engine: Arc<BI>,
	pub keystore: DKGKeystore,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub best_authorities: Arc<Vec<(KeygenPartyId, Public)>>,
	pub authority_public_key: Arc<Public>,
	pub party_i: KeygenPartyId,
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
	pub fn get_next_batch_key<MaxProposalLength: Get<u32> + Clone + Send + Sync>(
		&self,
		batch: &[UnsignedProposal<MaxProposalLength>],
	) -> BatchKey {
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
			party_i: self.party_i,
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

/// A Keygen Party Id, in the range [1, n]
///
/// This is a wrapper around u16 to ensure that the party id is in the range [1, n] and to prevent
/// the misuse of the party id as an offline party id, for example.
///
/// To construct a KeygenPartyId, use the `try_from` method.
#[derive(
	Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, codec::Encode, codec::Decode,
)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct KeygenPartyId(u16);

/// A Offline Party Id, in the range [1, t+1], where t is the signing threshold.
///
/// This is a wrapper around u16 to prevent the misuse of the party id as a keygen party id, for
/// example.
///
/// To construct a OfflinePartyId, use the [`Self::try_from_keygen_party_id`] method.
#[derive(
	Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, codec::Encode, codec::Decode,
)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct OfflinePartyId(u16);

impl TryFrom<u16> for KeygenPartyId {
	type Error = DKGError;
	/// This the only where you can construct a KeygenPartyId
	fn try_from(value: u16) -> Result<Self, Self::Error> {
		// party_i starts from 1
		if value == 0 {
			Err(DKGError::InvalidKeygenPartyId)
		} else {
			Ok(Self(value))
		}
	}
}

impl AsRef<u16> for KeygenPartyId {
	fn as_ref(&self) -> &u16 {
		&self.0
	}
}

impl fmt::Display for KeygenPartyId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl KeygenPartyId {
	/// Try to convert a KeygenPartyId to an OfflinePartyId.
	pub fn try_to_offline_party_id(&self, s_l: &[Self]) -> Result<OfflinePartyId, DKGError> {
		OfflinePartyId::try_from_keygen_party_id(*self, s_l)
	}

	/// Converts the PartyId to an index in the range [0, n-1].
	///
	/// The implementation is safe because the party id is guaranteed to be in the range [1, n].
	pub const fn to_index(&self) -> usize {
		self.0 as usize - 1
	}
}

impl AsRef<u16> for OfflinePartyId {
	fn as_ref(&self) -> &u16 {
		&self.0
	}
}

impl fmt::Display for OfflinePartyId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl OfflinePartyId {
	/// Creates a OfflinePartyId from a KeygenPartyId and a list of the signing parties.
	///
	/// This finds the index of the KeygenPartyId in the list of signing parties, then we use that
	/// index as OfflinePartyId.
	///
	/// This is safe because the KeygenPartyId is guaranteed to be in the range `[1, n]`, and the
	/// OfflinePartyId is guaranteed to be in the range `[1, t+1]`. if the KeygenPartyId is not in
	/// the list of signing parties, then we return an error.
	pub fn try_from_keygen_party_id(
		i: KeygenPartyId,
		s_l: &[KeygenPartyId],
	) -> Result<Self, DKGError> {
		// find the index of the party in the list of signing parties
		let index = s_l.iter().position(|&x| x == i).ok_or(DKGError::InvalidKeygenPartyId)?;
		let offline_id = index as u16 + 1;
		Ok(Self(offline_id))
	}

	/// Tries to Converts the `OfflinePartyId` to a `KeygenPartyId`.
	///
	/// Returns an error if the `OfflinePartyId` is not in the list of signing parties.
	pub fn try_to_keygen_party_id(&self, s_l: &[KeygenPartyId]) -> Result<KeygenPartyId, DKGError> {
		let idx = self.to_index();
		let party_i = s_l.get(idx).cloned().ok_or(DKGError::InvalidSigningSet)?;
		Ok(party_i)
	}

	/// Converts the OfflinePartyId to an index.
	pub const fn to_index(&self) -> usize {
		self.0 as usize - 1
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
pub enum ProtocolType<MaxProposalLength: Get<u32> + Clone + Send + Sync> {
	Keygen {
		ty: KeygenRound,
		i: KeygenPartyId,
		t: u16,
		n: u16,
	},
	Offline {
		unsigned_proposal: Arc<UnsignedProposal<MaxProposalLength>>,
		i: OfflinePartyId,
		s_l: Vec<KeygenPartyId>,
		local_key: Arc<LocalKey<Secp256k1>>,
	},
	Voting {
		offline_stage: Arc<CompletedOfflineStage>,
		unsigned_proposal: Arc<UnsignedProposal<MaxProposalLength>>,
		i: OfflinePartyId,
	},
}

impl<MaxProposalLength: Get<u32> + Clone + Send + Sync> ProtocolType<MaxProposalLength> {
	pub const fn get_i(&self) -> u16 {
		match self {
			Self::Keygen { i, .. } => i.0,
			Self::Offline { i, .. } => i.0,
			Self::Voting { i, .. } => i.0,
		}
	}
	pub fn get_unsigned_proposal(&self) -> Option<&UnsignedProposal<MaxProposalLength>> {
		match self {
			Self::Offline { unsigned_proposal, .. } | Self::Voting { unsigned_proposal, .. } =>
				Some(unsigned_proposal),
			_ => None,
		}
	}
}

impl<MaxProposalLength : Get<u32> + Clone +Send + Sync> Debug for ProtocolType<MaxProposalLength> {
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

pub fn new_inner<
	SM: StateMachineHandler + 'static,
	BI: BlockchainInterface + 'static,
	MaxProposalLength: Get<u32>  + Clone + Send + Sync + 'static,
>(
	additional_param: SM::AdditionalReturnParam,
	sm: SM,
	params: AsyncProtocolParameters<BI>,
	channel_type: ProtocolType<MaxProposalLength>,
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
			dkg_logging::info!(target: "dkg", "Running AsyncProtocol with party_index: {}", params.party_i);
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

fn generate_outgoing_to_wire_fn<
	SM: StateMachineHandler + 'static,
	BI: BlockchainInterface + 'static,
	MaxProposalLength: Get<u32> + Send + Sync + Clone + 'static,
>(
	params: AsyncProtocolParameters<BI>,
	outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>,
	proto_ty: ProtocolType<MaxProposalLength>,
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

			dkg_logging::info!(target: "dkg", "Async proto sent outbound request in session={} from={:?} to={:?} | (ty: {:?})", params.session_id, unsigned_message.sender, unsigned_message.receiver, &proto_ty);
			let party_id = unsigned_message.sender;
			let serialized_body = match serde_json::to_vec(&unsigned_message) {
				Ok(value) => value,
				Err(err) => {
					dkg_logging::error!(target: "dkg", "Failed to serialize message: {:?}, Skipping..", err);
					continue
				},
			};

			// we need to calculate the recipient id from the receiver.
			let maybe_recipient_id = match unsigned_message.receiver {
				Some(party_i) => {
					// Here we need to calculate the authority id of the recipient
					// using the KeygenPartyId.
					let keygen_party_id = KeygenPartyId::try_from(party_i)
						.expect("message receiver should be a valid KeygenPartyId");
					// try to find the authority id in the list of authorities by the
					// KeygenPartyId
					params.best_authorities.iter().find_map(|(id, p)| {
						if id == &keygen_party_id {
							Some(p.clone())
						} else {
							None
						}
					})
				},
				None => None,
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

			let id = params.authority_public_key.as_ref().clone();
			let unsigned_dkg_message = DKGMessage {
				sender_id: id,
				recipient_id: maybe_recipient_id,
				status,
				payload,
				session_id: params.session_id,
			};
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
	MaxProposalLength: Get<u32>  + Clone + Send + Sync + 'static,
>(
	params: AsyncProtocolParameters<BI>,
	channel_type: ProtocolType<MaxProposalLength>,
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

#[cfg(test)]
mod tests {
	use dkg_primitives::crypto::AuthorityId;
	use sp_application_crypto::ByteArray;

	use super::*;

	/// The Original Implementation of the Offline Index
	fn get_offline_stage_index(s_l: &[u16], keygen_party_idx: u16) -> Option<u16> {
		(1..)
			.zip(s_l)
			.find(|(_i, keygen_i)| keygen_party_idx == **keygen_i)
			.map(|r| r.0)
	}

	#[test]
	fn should_create_keygen_id_from_u16() {
		let party_id = 1;
		assert!(KeygenPartyId::try_from(party_id).is_ok());
		let party_id = 2;
		assert!(KeygenPartyId::try_from(party_id).is_ok());
		let party_id = 0;
		assert!(KeygenPartyId::try_from(party_id).is_err());
	}

	#[test]
	fn should_create_offline_id_from_keygen_id() {
		let party_id = 1;
		let keygen_id = KeygenPartyId::try_from(party_id).unwrap();
		let s_l = (1..=3).into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(keygen_id, &s_l).unwrap();
		assert_eq!(*offline_id.as_ref(), 1);
		assert_eq!(offline_id.to_index(), 0);
		let s_l = vec![2, 3, 1].into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(keygen_id, &s_l).unwrap();
		assert_eq!(*offline_id.as_ref(), 3);
		assert_eq!(offline_id.to_index(), 2);
	}

	#[test]
	fn should_return_the_correct_offline_id() {
		let party_id = 1;
		let keygen_id = KeygenPartyId::try_from(party_id).unwrap();
		let s_l = (1..=3).into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let s_l_raw = s_l.iter().map(|id| id.0).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(keygen_id, &s_l).unwrap();
		let expected_offline_id = get_offline_stage_index(&s_l_raw, party_id).unwrap();
		assert_eq!(*offline_id.as_ref(), expected_offline_id);

		let s_l = vec![2, 3, 1].into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let s_l_raw = s_l.iter().map(|id| id.0).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(keygen_id, &s_l).unwrap();
		let expected_offline_id = get_offline_stage_index(&s_l_raw, party_id).unwrap();
		assert_eq!(*offline_id.as_ref(), expected_offline_id);
	}

	#[test]
	fn should_convert_from_keygen_id_to_offline_id_and_back() {
		let party_id = 1;
		let orig_keygen_id = KeygenPartyId::try_from(party_id).unwrap();
		let s_l = (1..=3).into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(orig_keygen_id, &s_l).unwrap();
		let keygen_id = offline_id.try_to_keygen_party_id(&s_l).unwrap();
		assert_eq!(keygen_id, orig_keygen_id);
	}

	#[test]
	fn should_convert_offline_id_to_authority_id() {
		let authorities = vec![
			AuthorityId::from_slice(&[1; 33]),
			AuthorityId::from_slice(&[2; 33]),
			AuthorityId::from_slice(&[3; 33]),
			AuthorityId::from_slice(&[4; 33]),
		];
		let my_authority_id = AuthorityId::from_slice(&[2; 33]);
		let party_i = authorities
			.iter()
			.position(|id| id == &my_authority_id)
			.and_then(|i| u16::try_from(i + 1).ok())
			.unwrap();
		assert_eq!(party_i, 2);
		let keygen_id = KeygenPartyId::try_from(party_i).unwrap();
		let s_l = (1..=3).into_iter().map(KeygenPartyId).collect::<Vec<_>>();
		let offline_id = OfflinePartyId::try_from_keygen_party_id(keygen_id, &s_l).unwrap();
		assert_eq!(offline_id.to_index(), 1);
		assert_eq!(*offline_id.as_ref(), 2);

		// Convert offline id back to keygen id
		let my_keygen_id = offline_id.try_to_keygen_party_id(&s_l).unwrap();
		assert_eq!(my_keygen_id, keygen_id);
		// Convert keygen id to authority id
		let authority_id =
			authorities.get(my_keygen_id.to_index()).expect("authority id should exist");
		assert_eq!(authority_id, &my_authority_id);
	}
}
