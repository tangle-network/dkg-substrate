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


use std::{task::{Context, Poll}, pin::Pin};
use std::sync::Arc;

use codec::Encode;
use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{types::{DKGError, DKGMessage, SignedDKGMessage, DKGMsgPayload}, crypto::Public};
use dkg_runtime_primitives::utils::to_slice_32;
use futures::{stream::Stream, Sink, TryStreamExt};
use log::error;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{LocalKey, ProtocolMessage};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::OfflineProtocolMessage;
use round_based::Msg;
use sp_core::sr25519;
use sp_runtime::{traits::Block, AccountId32};
use tokio_stream::wrappers::BroadcastStream;

pub type SignedMessageReceiver = tokio::sync::broadcast::Receiver<Arc<SignedDKGMessage<Public>>>;

pub struct IncomingAsyncProtocolWrapper<T: TransformIncoming> {
    pub receiver: BroadcastStream<T>,
	ty: ProtocolType
}

impl<T: TransformIncoming> IncomingAsyncProtocolWrapper<T> {
	pub fn new(receiver: tokio::sync::broadcast::Receiver<T>, ty: ProtocolType) -> Self {
		Self {
			receiver: BroadcastStream::new(receiver),
			ty
		}
	}
}

pub struct OutgoingAsyncProtocolWrapper<T: TransformOutgoing> {
    pub sender: futures::channel::mpsc::UnboundedSender<T::Output>,
}

#[derive(Debug, Clone)]
enum ProtocolType {
	Keygen { i: u16, t: u16, n: u16 },
	Offline { i: u16, s_l: Vec<u16>, local_key: LocalKey<Secp256k1> },
	Voting
}

enum ProtocolMessageType {
	Keygen(ProtocolMessage),
	Offline(OfflineProtocolMessage)
}

pub trait TransformIncoming: Clone + Send + 'static {
    type IncomingMapped;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32], stream_type: &ProtocolType) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError> where Self: Sized;
}

impl TransformIncoming for Arc<SignedDKGMessage<Public>> {
    type IncomingMapped = DKGMessage<Public>;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32], stream_type: &ProtocolType) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError> where Self: Sized {
        verify_signature_against_authorities(&*self, active, next).map(|body| {
			match (stream_type, &body.payload) {
				(ProtocolType::Keygen { .. }, DKGMsgPayload::Keygen(..)) |
				(ProtocolType::Offline { .. }, DKGMsgPayload::Offline(..)) |
				(ProtocolType::Voting, DKGMsgPayload::Vote(..)) => {
					// only clone if the downstream receiver expects this type
					Some(Msg { sender: party_index, receiver: None, body: body.clone() })
				}

				_ => None
			}
        })
    }
}

/// Check a `signature` of some `data` against a `set` of accounts.
/// Returns true if the signature was produced from an account in the set and false otherwise.
fn check_signers(data: &[u8], signature: &[u8], set: &[AccountId32]) -> Result<bool, DKGError> {
	let maybe_signers = set.iter()
		.map(|x| {
			let val = x.encode();
			let slice = to_slice_32(&val).ok_or_else(|| DKGError::GenericError { reason: "Failed to convert account id to sr25519 public key".to_string() })?;
			Ok(sr25519::Public(slice))
		}).collect::<Vec<Result<sr25519::Public, DKGError>>>();

	let mut processed = vec![];

	for maybe_signer in maybe_signers {
		processed.push(maybe_signer?);
	}

    let is_okay = dkg_runtime_primitives::utils::verify_signer_from_set(
        processed,
		data,
        signature,
    ).1;

	Ok(is_okay)
}

/// Verifies a SignedDKGMessage was signed by the active or next authorities
fn verify_signature_against_authorities<'a>(
    signed_dkg_msg: &'a SignedDKGMessage<Public>,
    active_authorities: &'a [AccountId32],
    next_authorities: &'a [AccountId32],
) -> Result<&'a DKGMessage<Public>, DKGError> {
    let dkg_msg = &signed_dkg_msg.msg;
    let encoded = dkg_msg.encode();
    let signature = signed_dkg_msg.signature.unwrap_or_default();

	let res0 = check_signers(&encoded, &signature, active_authorities);
	let res1 = check_signers(&encoded, &signature, next_authorities);

	match (res0, res1) {
		(Ok(true), _) | (_, Ok(true)) => Ok(dkg_msg),
		_ => {
			Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority or next authority"
					.into(),
			})
		}
	}
}


pub trait TransformOutgoing {
    type Output;
    fn transform(self) -> Result<Self::Output, DKGError> where Self: Sized;
}

impl<T> Stream for IncomingAsyncProtocolWrapper<T>
where
    T: TransformIncoming,
{
    type Item = Msg<T::IncomingMapped>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        match futures::ready!(Pin::new(&mut self.receiver).poll_next(cx)) {
            Some(Ok(msg)) => {
				let ty = &self.ty;
                match msg.transform(ty) {
					Ok(Some(msg)) => {
						Poll::Ready(Some(msg))
					}

					Ok(None) => {
						Poll::Pending
					}

					Err(err) => {
						log::warn!(target: "dkg", "While mapping signed message, received an error: {:?}", err);
						Poll::Pending
					}
				}
            },
			Some(Err(err)) => {
				log::error!(target: "dkg", "Stream RECV error: {:?}", err);
				Poll::Ready(None)
			}
            None => Poll::Ready(None),
        }
    }
}

impl<T> Sink<Msg<T>> for OutgoingAsyncProtocolWrapper<T>
where
    T: TransformOutgoing
{
    type Error = DKGError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_ready(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }

    fn start_send(mut self: Pin<&mut Self>, item: Msg<T>) -> Result<(), Self::Error> {
        let transformed_item = item.body.transform();
        match transformed_item {
            Ok(item) => Pin::new(&mut self.sender).start_send(item).map_err(|err| DKGError::GenericError { reason: err.to_string() }),
            Err(err) => {
                // log the error
                error!(target: "dkg", "🕸️  Failed to transform outgoing message: {:?}", err);
                Ok(())
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_flush(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_close(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }
}

pub mod meta_channel {
	use std::future::Future;
	use std::pin::Pin;
	use std::sync::Arc;
	use std::task::{Context, Poll};
	use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
	use parking_lot::Mutex;
	use futures::{select, StreamExt};
	use log::debug;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{Keygen, ProtocolMessage};
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{OfflineProtocolMessage, OfflineStage};
	use round_based::async_runtime::watcher::StderrWatcher;
	use round_based::{AsyncProtocol, Msg, StateMachine};
	use round_based::containers::StoreErr;
	use sc_network_gossip::GossipEngine;
	use serde::Serialize;
	use sp_runtime::traits::Block;
	use dkg_primitives::Public;
	use dkg_primitives::types::{DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage, DKGVoteMessage};
	use crate::async_protocol_handlers::{IncomingAsyncProtocolWrapper, ProtocolType, SignedMessageReceiver};
	use crate::DKGKeystore;
	use crate::messages::dkg_message::sign_and_send_messages;

	pub trait SendFuture: Future<Output=Result<(), DKGError>> + Send {}
	impl<T> SendFuture for T where T: Future<Output=Result<(), DKGError>> + Send {}

	/// Once created, the MetaDKGMessageHandler should be .awaited to begin execution
	pub struct MetaDKGMessageHandler {
		protocol: Pin<Box<dyn SendFuture>>
	}

	trait StateMachineIface: StateMachine + Send {
		fn generate_channel() -> (futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, futures::channel::mpsc::UnboundedReceiver<Msg<<Self as StateMachine>::MessageBody>>) {
			futures::channel::mpsc::unbounded()
		}

		fn handle_unsigned_message(to_async_proto: &futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err>;
	}

	impl StateMachineIface for Keygen {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<ProtocolMessage>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { id, payload, round_id } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Keygen(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Error as Error;
					let message: Msg<ProtocolMessage> = serde_json::from_slice(msg.keygen_msg.as_slice()).map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;
					to_async_proto.unbounded_send(message).map_err(|err| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}
	}

	impl StateMachineIface for OfflineStage {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { id, payload, round_id } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Offline(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::Error as Error;
					let message: Msg<OfflineProtocolMessage> = serde_json::from_slice(msg.offline_msg.as_slice()).map_err(|err| Error::HandleMessage(StoreErr::NotForMe))?;
					to_async_proto.unbounded_send(message).map_err(|err| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}
	}

	impl MetaDKGMessageHandler {
		pub fn new<B>(gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore, signed_message_receiver: SignedMessageReceiver, channel_type: ProtocolType) -> Result<Self, DKGError>
				where
		          B: Block {
			match channel_type.clone() {
				ProtocolType::Keygen { i, t, n } => {
					Self::new_inner(Keygen::new(i, t, n).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, signed_message_receiver, channel_type, gossip_engine, keystore)
				}
				ProtocolType::Offline { i, s_l, local_key } => {
					Self::new_inner(OfflineStage::new(i, s_l, local_key).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, signed_message_receiver, channel_type, gossip_engine, keystore)
				}
				ProtocolType::Voting => {
					Self::new_voting(signed_message_receiver, gossip_engine, keystore)
				}
			}
		}

		fn new_inner<B: Block, SM: StateMachineIface>(sm: SM, signed_message_receiver: SignedMessageReceiver, channel_type: ProtocolType, gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore) -> Result<Self, DKGError> {
			let (incoming_tx_proto, incoming_rx_proto) = SM::generate_channel();
			let (outgoing_tx, mut outgoing_rx) = futures::channel::mpsc::unbounded();

			let async_proto = AsyncProtocol::new(sm, incoming_rx_proto.map(Ok), outgoing_tx.clone()).run();

			// For taking all unsigned messages generated by the AsyncProtocols, signing them,
			// and thereafter sending them outbound
			let outgoing_to_wire = Self::generate_outgoing_to_wire_fn(gossip_engine, keystore, outgoing_rx, channel_type.clone());

			// For taking raw inbound signed messages, mapping them to unsigned messages, then sending
			// to the appropriate AsyncProtocol
			let inbound_signed_message_receiver = Self::generate_inbound_signed_message_receiver_fn(signed_message_receiver, channel_type, incoming_tx_proto);

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				select! {
					proto_res = async_proto => {
						log::error!(target: "dkg", "🕸️  Protocol {:?} Ended: {:?}", channel_type, proto_res);
						proto_res
					},

					outgoing_res = outgoing_to_wire => {
						log::error!(target: "dkg", "🕸️  Outbound Sender Ended: {:?}", outgoing_res);
						outgoing_res
					},

					incoming_res = inbound_signed_message_receiver => {
						log::error!(target: "dkg", "🕸️  Inbound Receiver Ended: {:?}", incoming_res);
						incoming_res
					}
				}
			};


			Ok(Self {
				protocol: Box::pin(protocol)
			})
		}

		fn new_voting<B: Block>(signed_message_receiver: SignedMessageReceiver, gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore) -> Result<Self, DKGError> {
			let protocol = Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(signed_message_receiver, ProtocolType::Voting);

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					// TODO: handle entirely here, including signing + sending too wire
				}

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			});

			Ok(Self {
				protocol
			})
		}

		fn generate_outgoing_to_wire_fn<B: Block, SM: StateMachineIface>(ref gossip_engine: Arc<Mutex<GossipEngine<B>>>, ref keystore: DKGKeystore, mut outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>, ref proto_ty: ProtocolType) -> Pin<Box<dyn Future<Output=Result<(), DKGError>>>>
			where <SM as StateMachine>::MessageBody: Serialize {
			 Box::pin(async move {
				// take all unsigned messages, then sign them and send outbound
				while let Some(unsigned_message) = outgoing_rx.next().await {
					let serialized_body = serde_json::to_vec(&unsigned_message).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
					let round_id = 0;
					let id = Default::default();

					let payload = match proto_ty {
						ProtocolType::Keygen { .. } => {
							DKGMsgPayload::Keygen(DKGKeygenMessage { round_id, keygen_msg: serialized_body })
						}
						ProtocolType::Offline { .. } => {
							DKGMsgPayload::Offline(DKGOfflineMessage {
								key: vec![],
								signer_set_id: 0,
								offline_msg: serialized_body
							})
						}
						_ => {
							unreachable!("Should not happen since voting is handled with a custom subroutine")
						}
					};

					let unsigned_dkg_message = DKGMessage { id, payload, round_id };
					sign_and_send_messages(gossip_engine,keystore, unsigned_dkg_message);
				}

				Err(DKGError::CriticalError { reason: "Outbound stream stopped producing items".to_string() })
			})
		}

		fn generate_inbound_signed_message_receiver_fn<SM: StateMachineIface>(signed_message_receiver: SignedMessageReceiver, channel_type: ProtocolType, ref to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>) -> Pin<Box<dyn Future<Output=Result<(), DKGError>>>> {
			Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(signed_message_receiver, channel_type);

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					SM::handle_unsigned_message(to_async_proto, unsigned_message)?;
				}

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			})
		}
	}

	impl Future for MetaDKGMessageHandler {
		type Output = Result<(), DKGError>;

		fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
			self.protocol.as_mut().poll(cx)
		}
	}
}
