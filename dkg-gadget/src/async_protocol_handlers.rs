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

use codec::Encode;
use dkg_primitives::{types::{DKGError, DKGMessage, SignedDKGMessage, DKGMsgPayload}, crypto::Public, AuthoritySet};
use dkg_runtime_primitives::utils::to_slice_32;
use futures::{stream::Stream, Sink, TryStreamExt};
use log::error;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{keygen::Keygen};
use round_based::{Msg, async_runtime::AsyncProtocol, StateMachine, IsCritical};
use sp_core::sr25519;
use sp_runtime::{traits::Block, AccountId32};
use sc_client_api::Backend;
use crate::Client;

use crate::worker::DKGWorker;

use self::state_machine::DKGStateMachine;

pub struct IncomingAsyncProtocolWrapper<T> {
    pub receiver: futures::channel::mpsc::UnboundedReceiver<T>,
}

pub struct OutgoingAsyncProtocolWrapper<T: TransformOutgoing> {
    pub sender: futures::channel::mpsc::UnboundedSender<T::Output>,
}

pub trait TransformIncoming {
    type IncomingMapped;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32]) -> Result<Msg<Self::IncomingMapped>, DKGError> where Self: Sized;
}

impl TransformIncoming for SignedDKGMessage<Public> {
    type IncomingMapped = DKGMessage<Public>;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32]) -> Result<Msg<Self::IncomingMapped>, DKGError> where Self: Sized {
        verify_signature_against_authorities(self, active, next).map(|body| {
            Msg { sender: party_index, receiver: None, body }
        })
    }
}

/// Check a `signature` of some `data` against a `set` of accounts.
/// Returns true if the signature was produced from an account in the set and false otherwise.
fn check_signers(data: &[u8], signature: &[u8], set: &[AccountId32]) -> Result<bool, DKGError> {
	let maybe_signers = set.iter()
		.map(|x| {
			let val = x.encode().map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
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
fn verify_signature_against_authorities(
    signed_dkg_msg: SignedDKGMessage<Public>,
    active_authorities: &[AccountId32],
    next_authorities: &[AccountId32],
) -> Result<DKGMessage<Public>, DKGError> {
    let dkg_msg = signed_dkg_msg.msg;
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
    type Item = Result<Msg<T>, DKGError>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        match futures::ready!(Pin::new(&mut self.receiver).poll_next(cx)) {
            Some(msg) => {
                match msg.transform() {
                    Ok(msg) => Poll::Ready(Some(Ok(msg))),
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
        let transformed_item = item.body.transform(); // result<T::Output>
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
	use round_based::async_runtime::watcher::StderrWatcher;
	use round_based::AsyncProtocol;
	use sc_network_gossip::GossipEngine;
	use sp_runtime::traits::Block;
	use dkg_primitives::Public;
	use dkg_primitives::types::{DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage, DKGVoteMessage, SignedDKGMessage};
	use dkg_runtime_primitives::crypto::AuthorityId;
	use crate::async_protocol_handlers::IncomingAsyncProtocolWrapper;
	use crate::async_protocol_handlers::state_machines::{KeygenStateMachine, OfflineStateMachine, VotingStateMachine};
	use crate::DKGKeystore;
	use crate::messages::dkg_message::sign_and_send_messages;

	pub trait SendFuture: Future<Output=Result<(), DKGError>> + Send {}
	impl<T> SendFuture for T where T: Future<Output=Result<(), DKGError>> + Send {}

	/// Once created, the MetaDKGMessageHandler should be .awaited to begin execution
	pub struct MetaDKGMessageHandler {
		protocol: Pin<Box<dyn SendFuture>>
	}

	impl MetaDKGMessageHandler {
		/// `to_outbound_wire` must be a function that takes DKGMessages and sends them outbound to
		/// the internet
		pub fn new<B>(gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore, signed_message_receiver: UnboundedReceiver<SignedDKGMessage<Public>>) -> Self
				where
		          B: Block {

			let (incoming_tx_keygen, incoming_rx_keygen) = futures::channel::mpsc::unbounded();
			let (incoming_tx_offline, incoming_rx_offline) = futures::channel::mpsc::unbounded();
			let (incoming_tx_vote, incoming_rx_vote) = futures::channel::mpsc::unbounded();

			let (outgoing_tx, mut outgoing_rx) = futures::channel::mpsc::unbounded::<DKGMessage<Public>>();

			let keygen_state_machine = KeygenStateMachine { };
			let offline_state_machine = OfflineStateMachine { };
			let voting_state_machine = VotingStateMachine { };

			let ref mut keygen_proto = AsyncProtocol::new(keygen_state_machine, incoming_rx_keygen, outgoing_tx.clone()).set_watcher(StderrWatcher);
			let ref mut offline_proto = AsyncProtocol::new(offline_state_machine, incoming_rx_offline, outgoing_tx.clone()).set_watcher(StderrWatcher);
			let ref mut voting_proto = AsyncProtocol::new(voting_state_machine, incoming_rx_vote, outgoing_tx).set_watcher(StderrWatcher);

			// For taking all unsigned messages generated by the AsyncProtocols, signing them,
			// and thereafter sending them outbound
			let ref mut outgoing_to_wire = Box::pin(async move {
				// take all unsigned messages, then sign them and send outbound
				while let Some(unsigned_message) = outgoing_rx.next().await {
					sign_and_send_messages(&gossip_engine,&keystore, unsigned_message).await;
				}

				Err(DKGError::CriticalError { reason: "Outbound stream stopped producing items".to_string() })
			});

			// For taking raw inbound signed messages, mapping them to unsigned messages, then sending
			// to the appropriate AsyncProtocol
			let ref mut inbound_signed_message_receiver = Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper { receiver: signed_message_receiver };

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					match unsigned_message {
						Ok(msg) => {
							let DKGMessage { id, payload, round_id } = msg;

							// Send the payload to the appropriate AsyncProtocols
							match payload {
								DKGMsgPayload::Keygen(msg) => incoming_tx_keygen.unbounded_send(msg).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
								DKGMsgPayload::Offline(msg) => incoming_tx_offline.unbounded_send(msg).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
								DKGMsgPayload::Vote(msg) => incoming_tx_vote.unbounded_send(msg).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?,
								err => debug!(target: dkg, "Invalid payload received: {:?}", err)
							}
						}

						Err(err) => {
							debug!(target: dkg, "Invalid signed DKG message received: {:?}", err)
						}
					}
				}

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			});

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				select! {
					keygen_res = keygen_proto.run() => {
						error!(target: "dkg", "🕸️  Keygen Protocol Ended: {:?}", keygen_res);
						keygen_res
					},

					offline_res = offline_proto.run() => {
						error!(target: "dkg", "🕸️ Offline Protocol Ended: {:?}", offline_res);
						offline_res
					},

					voting_res = voting_proto.run() => {
						error!(target: "dkg", "🕸️  Voting Protocol Ended: {:?}", voting_res);
						voting_res
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


			Self {
				protocol: Box::pin(protocol)
			}
		}
	}

	impl Future for MetaDKGMessageHandler {
		type Output = Result<(), DKGError>;

		fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
			self.protocol.as_mut().poll(cx)
		}
	}
}

pub mod state_machines {
	use std::time::Duration;
	use futures::channel::mpsc::UnboundedReceiver;
	use round_based::{Msg, StateMachine};
	use dkg_primitives::types::{DKGError, DKGKeygenMessage};

	pub struct KeygenStateMachine {

	}

	pub struct OfflineStateMachine {

	}

	pub struct VotingStateMachine {

	}

	impl StateMachine for KeygenStateMachine {
		type MessageBody = DKGKeygenMessage;
		type Err = DKGError;
		type Output = ();

		fn handle_incoming(&mut self, msg: Msg<Self::MessageBody>) -> Result<(), Self::Err> {
			msg.body.
		}

		fn message_queue(&mut self) -> &mut Vec<Msg<Self::MessageBody>> {
			todo!()
		}

		fn wants_to_proceed(&self) -> bool {
			todo!()
		}

		fn proceed(&mut self) -> Result<(), Self::Err> {
			todo!()
		}

		fn round_timeout(&self) -> Option<Duration> {
			todo!()
		}

		fn round_timeout_reached(&mut self) -> Self::Err {
			todo!()
		}

		fn is_finished(&self) -> bool {
			todo!()
		}

		fn pick_output(&mut self) -> Option<Result<Self::Output, Self::Err>> {
			todo!()
		}

		fn current_round(&self) -> u16 {
			todo!()
		}

		fn total_rounds(&self) -> Option<u16> {
			todo!()
		}

		fn party_ind(&self) -> u16 {
			todo!()
		}

		fn parties(&self) -> u16 {
			todo!()
		}
	}
}

impl IsCritical for DKGError {
	/// Indicates whether an error critical or not
	fn is_critical(&self) -> bool {
		match self {
			DKGError::CriticalError { .. } => true,
			_ => false
		}
	}
}
