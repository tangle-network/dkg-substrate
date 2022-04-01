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
use std::marker::PhantomData;
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

use self::state_machine::DKGStateMachine;

pub type SignedMessageReceiver = tokio::sync::broadcast::Receiver<Arc<SignedDKGMessage<Public>>>;

pub struct IncomingAsyncProtocolWrapper<T> {
    pub receiver: SignedMessageReceiver,
	ty: ProtocolType,
	_pd: PhantomData<T>
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

pub trait TransformIncoming {
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
		collect(),
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

    if check_signers(&encoded, &signature, active_authorities) == Ok(true) || check_signers(&encoded, &signature, next_authorities) == Ok(true) {
        Ok(dkg_msg)
    } else {
        Err(DKGError::GenericError {
            reason: "Message signature is not from a registered authority or next authority"
                .into(),
        })
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
    type Item = Result<Option<Msg<T>>, DKGError>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        match futures::ready!(Pin::new(&mut self.receiver).poll_next(cx)) {
            Some(msg) => {
				let ty = &self.ty;
                match msg.transform(ty) {
                    Ok(res) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            },
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
	use futures::lock::Mutex;
	use futures::{select, StreamExt};
	use log::debug;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{Keygen, ProtocolMessage};
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{OfflineProtocolMessage, OfflineStage};
	use round_based::async_runtime::watcher::StderrWatcher;
	use round_based::{AsyncProtocol, Msg, StateMachine};
	use sc_network_gossip::GossipEngine;
	use sp_runtime::traits::Block;
	use dkg_primitives::Public;
	use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload};
	use crate::async_protocol_handlers::{IncomingAsyncProtocolWrapper, ProtocolType, SignedMessageReceiver};
	use crate::async_protocol_handlers::state_machines::{KeygenStateMachine, OfflineStateMachine, VotingStateMachine};
	use crate::DKGKeystore;
	use crate::messages::dkg_message::sign_and_send_messages;

	pub trait SendFuture: Future<Output=Result<(), DKGError>> + Send {}
	impl<T> SendFuture for T where T: Future<Output=Result<(), DKGError>> + Send {}

	/// Once created, the MetaDKGMessageHandler should be .awaited to begin execution
	pub struct MetaDKGMessageHandler {
		protocol: Pin<Box<dyn SendFuture>>
	}

	trait StateMachineIface: StateMachine {
		fn generate_channel() -> (futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, futures::channel::mpsc::UnboundedReceiver<Msg<<Self as StateMachine>::MessageBody>>) {
			futures::channel::mpsc::unbounded()
		}

		fn handle_unsigned_message(to_async_proto: &futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, msg: DKGMessage<Public>) -> Result<(), DKGError>;
	}

	impl StateMachineIface for Keygen {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<ProtocolMessage>>, msg: DKGMessage<Public>) -> Result<(), DKGError> {
			let DKGMessage { id, payload, round_id } = msg;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Keygen(msg) => {
					let message: Msg<ProtocolMessage> = serde_json::from_slice(msg.keygen_msg.as_slice()).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
					to_async_proto.unbounded_send(message).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}
	}

	impl StateMachineIface for OfflineStage {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>, msg: DKGMessage<Public>) -> Result<(), DKGError> {
			let DKGMessage { id, payload, round_id } = msg;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Offline(msg) => {
					let message: Msg<OfflineProtocolMessage> = serde_json::from_slice(msg.offline_msg.as_slice()).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
					to_async_proto.unbounded_send(message).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
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
			let (outgoing_tx, mut outgoing_rx) = futures::channel::mpsc::unbounded::<DKGMessage<Public>>();

			let ref mut async_proto = AsyncProtocol::new(sm, incoming_rx_proto, outgoing_tx.clone()).set_watcher(StderrWatcher);

			// For taking all unsigned messages generated by the AsyncProtocols, signing them,
			// and thereafter sending them outbound
			let ref mut outgoing_to_wire = Self::generate_outgoing_to_wire_fn(gossip_engine, keystore, outgoing_rx);

			// For taking raw inbound signed messages, mapping them to unsigned messages, then sending
			// to the appropriate AsyncProtocol
			let ref mut inbound_signed_message_receiver = Self::generate_inbound_signed_message_receiver_fn(signed_message_receiver, channel_type, incoming_tx_proto);

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				select! {
					proto_res = async_proto.run() => {
						error!(target: "dkg", "🕸️  Async Protocol {:?} Ended: {:?}", channel_type, keygen_res);
						proto_res
					},

					outgoing_res = outgoing_to_wire => {
						error!(target: "dkg", "🕸️  Outbound Sender Ended: {:?}", outgoing_res);
						outgoing_res
					},

					incoming_res = inbound_signed_message_receiver => {
						error!(target: "dkg", "🕸️  Inbound Receiver Ended: {:?}", incoming_res);
						incoming_res
					}
				}
			};


			Ok(Self {
				protocol: Box::pin(protocol)
			})
		}

		fn new_voting<B: Block>(signed_message_receiver: SignedMessageReceiver, gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore) -> Result<Self, DKGError> {
			let (outgoing_tx, mut outgoing_rx) = futures::channel::mpsc::unbounded::<DKGMessage<Public>>();

			let ref mut outgoing_to_wire = Self::generate_outgoing_to_wire_fn(gossip_engine, keystore, outgoing_rx);

			let ref mut inbound_signed_message_receiver = Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper { receiver: signed_message_receiver, ty: ProtocolType::Voting, _pd: Default::default() };

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					match unsigned_message {
						Ok(Some(msg)) => {
							// instead of sending to async protocol, send straight to outbound channel to be signed and sent
							outgoing_tx.unbounded_send(msg.body).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?;
						}

						Ok(None) => {
							// do nothing. This message was not meant for this handler
						}

						Err(err) => {
							debug!(target: "dkg", "Invalid signed DKG message received: {:?}", err)
						}
					}
				}

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			});

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				select! {
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

		fn generate_outgoing_to_wire_fn<B: Block, SM: StateMachineIface>(ref gossip_engine: Arc<Mutex<GossipEngine<B>>>, ref keystore: DKGKeystore, mut outgoing_rx: UnboundedReceiver<DKGMessage<Public>>) -> Pin<Box<dyn Future<Output=Result<(), DKGError>>>> {
			 Box::pin(async move {
				// take all unsigned messages, then sign them and send outbound
				while let Some(unsigned_message) = outgoing_rx.next().await {
					sign_and_send_messages(gossip_engine,keystore, unsigned_message);
				}

				Err(DKGError::CriticalError { reason: "Outbound stream stopped producing items".to_string() })
			})
		}

		fn generate_inbound_signed_message_receiver_fn<SM: StateMachineIface>(signed_message_receiver: SignedMessageReceiver, channel_type: ProtocolType, ref to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>) -> Pin<Box<dyn Future<Output=Result<(), DKGError>>>> {
			Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper { receiver: signed_message_receiver, ty: channel_type, _pd: Default::default() };

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					match unsigned_message {
						Ok(Some(msg)) => {
							SM::handle_unsigned_message(to_async_proto, msg)?;
						}

						Ok(None) => {
							// do nothing. This message was not meant for this handler
						}

						Err(err) => {
							debug!(target: "dkg", "Invalid signed DKG message received: {:?}", err)
						}
					}
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
