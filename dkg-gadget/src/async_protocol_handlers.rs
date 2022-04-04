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
use dkg_primitives::types::{DKGError, DKGMessage, SignedDKGMessage, DKGMsgPayload};
use dkg_runtime_primitives::utils::to_slice_32;
use futures::{stream::Stream, Sink};
use log::error;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{LocalKey, ProtocolMessage};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::OfflineProtocolMessage;
use round_based::Msg;
use sp_core::sr25519;
use sp_runtime::AccountId32;
use tokio_stream::wrappers::BroadcastStream;
use dkg_runtime_primitives::crypto::Public;

pub type SignedMessageReceiver = tokio::sync::broadcast::Receiver<Arc<SignedDKGMessage<Public>>>;

pub trait VerifyFn<T: TransformIncoming>: FnMut(T) -> Result<(T::IncomingMapped, PartyIndex), DKGError> + Send {}
impl<T: TransformIncoming, F: FnMut(T) -> Result<(T::IncomingMapped, PartyIndex), DKGError> + Send> VerifyFn<T> for F {}

pub struct IncomingAsyncProtocolWrapper<T: TransformIncoming> {
    pub receiver: BroadcastStream<T>,
	verification_function: Box<dyn VerifyFn<T>>,
	ty: ProtocolType
}

impl<T: TransformIncoming> IncomingAsyncProtocolWrapper<T> {
	pub fn new(receiver: tokio::sync::broadcast::Receiver<T>,
			   ty: ProtocolType,
			   verification_function: Box<dyn VerifyFn<T>>) -> Self {
		Self {
			receiver: BroadcastStream::new(receiver),
			verification_function,
			ty
		}
	}
}

pub struct OutgoingAsyncProtocolWrapper<T: TransformOutgoing> {
    pub sender: futures::channel::mpsc::UnboundedSender<T::Output>,
}

#[derive(Debug, Clone)]
pub enum ProtocolType {
	Keygen { i: u16, t: u16, n: u16 },
	Offline { i: u16, s_l: Vec<u16>, local_key: LocalKey<Secp256k1> },
	Voting
}

enum ProtocolMessageType {
	Keygen(ProtocolMessage),
	Offline(OfflineProtocolMessage)
}

pub type PartyIndex = u16;

pub trait TransformIncoming: Clone + Send + 'static {
    type IncomingMapped;
    fn transform(self, verify: impl VerifyFn<Self>, stream_type: &ProtocolType) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError> where Self: Sized;
}

impl TransformIncoming for Arc<SignedDKGMessage<Public>> {
    type IncomingMapped = DKGMessage<Public>;
    fn transform(self, mut verify: impl VerifyFn<Self>, stream_type: &ProtocolType) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError> where Self: Sized {
		verify(self).map(|(body, party_index)| {
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
		let Self { receiver, ty, verification_function } = &mut *self;

        match futures::ready!(Pin::new(receiver).poll_next(cx)) {
            Some(Ok(msg)) => {
                match msg.transform(verification_function, &*ty) {
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
	use std::fmt::Debug;
	use std::future::Future;
	use std::marker::PhantomData;
	use std::pin::Pin;
	use std::sync::Arc;
	use std::task::{Context, Poll};
	use curv::arithmetic::Converter;
	use curv::BigInt;
	use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
	use parking_lot::{Mutex, RwLock};
	use futures::{select, StreamExt, TryFutureExt};
	use log::debug;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{Keygen, ProtocolMessage};
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{OfflineProtocolMessage, OfflineStage, SignManual};
	use round_based::async_runtime::watcher::StderrWatcher;
	use round_based::{AsyncProtocol, Msg, StateMachine};
	use round_based::containers::StoreErr;
	use sc_client_api::Backend;
	use sc_network_gossip::GossipEngine;
	use serde::Serialize;
	use dkg_runtime_primitives::DKGApi;
	use sp_runtime::traits::{Block, Header};
	use dkg_runtime_primitives::AuthoritySet;
	use dkg_runtime_primitives::crypto::Public;
	use dkg_primitives::types::{DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage, DKGVoteMessage, SignedDKGMessage};
	use dkg_runtime_primitives::crypto::AuthorityId;
	use crate::async_protocol_handlers::{IncomingAsyncProtocolWrapper, ProtocolType, SignedMessageReceiver, VerifyFn};
	use crate::{Client, DKGKeystore};
	use crate::messages::dkg_message::sign_and_send_messages;
	use crate::utils::find_index;
	use crate::worker::{AsyncProtocolParameters, DKGWorker};

	pub trait SendFuture: Future<Output=Result<(), DKGError>> + Send + 'static {}
	impl<T> SendFuture for T where T: Future<Output=Result<(), DKGError>> + Send + 'static {}

	/// Once created, the MetaDKGMessageHandler should be .awaited to begin execution
	pub struct MetaDKGMessageHandler<B, BE, C> {
		protocol: Pin<Box<dyn SendFuture>>,
		_pd: PhantomData<(B, BE, C)>
	}

	pub struct DKGParamsForAsyncProto<B: Block> {
		gossip_engine: Arc<Mutex<GossipEngine<B>>>,
		keystore: DKGKeystore
	}

	trait StateMachineIface: StateMachine + Send + 'static {
		fn generate_channel() -> (futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, futures::channel::mpsc::UnboundedReceiver<Msg<<Self as StateMachine>::MessageBody>>) {
			futures::channel::mpsc::unbounded()
		}

		fn handle_unsigned_message(to_async_proto: &futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err>;
	}

	impl StateMachineIface for Keygen {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<ProtocolMessage>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { payload, .. } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Keygen(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Error as Error;
					let message: Msg<ProtocolMessage> = serde_json::from_slice(msg.keygen_msg.as_slice()).map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;
					to_async_proto.unbounded_send(message).map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}
	}

	impl StateMachineIface for OfflineStage {
		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { payload, .. } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Offline(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::Error as Error;
					let message: Msg<OfflineProtocolMessage> = serde_json::from_slice(msg.offline_msg.as_slice()).map_err(|err| Error::HandleMessage(StoreErr::NotForMe))?;
					to_async_proto.unbounded_send(message).map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}
	}

	impl<B, BE, C> MetaDKGMessageHandler<B, BE, C> where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE> + 'static,
		C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number> {

		pub fn new(params: AsyncProtocolParameters<B, C>, channel_type: ProtocolType) -> Result<Self, DKGError> {
			match channel_type.clone() {
				ProtocolType::Keygen { i, t, n } => {
					Self::new_inner(Keygen::new(i, t, n).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, params, channel_type)
				}
				ProtocolType::Offline { i, s_l, local_key } => {
					Self::new_inner(OfflineStage::new(i, s_l, local_key).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, params, channel_type)
				}
				ProtocolType::Voting => {
					unimplemented!("Voting not yet implemented")
					//Self::new_voting(params)
				}
			}
		}

		fn new_inner<SM: StateMachineIface>(sm: SM, params: AsyncProtocolParameters<B, C>, channel_type: ProtocolType) -> Result<Self, DKGError>
			where <SM as StateMachine>::Err: Send + Debug,
				  <SM as StateMachine>::MessageBody: std::marker::Send,
				  <SM as StateMachine>::MessageBody: Serialize {
			use futures::FutureExt;

			let (incoming_tx_proto, incoming_rx_proto) = SM::generate_channel();
			let (outgoing_tx, outgoing_rx) = futures::channel::mpsc::unbounded();

			let mut async_proto = AsyncProtocol::new(sm, incoming_rx_proto.map(Ok::<_, <SM as StateMachine>::Err>), outgoing_tx.clone())
				.set_watcher(StderrWatcher);

			let async_proto = Box::pin(async move {
				async_proto.run().await
					.map_err(|err| DKGError::GenericError { reason: format!("{:?}", err) })
					.map(|_| ())
			});

			let gossip_engine = params.gossip_engine.clone();
			let keystore = params.keystore.clone();
			let current_authority_set = params.current_validator_set.clone();
			let best_authorities = params.best_authorities;
			let authority_public_key = params.authority_public_key;

			// For taking all unsigned messages generated by the AsyncProtocols, signing them,
			// and thereafter sending them outbound
			let outgoing_to_wire = Self::generate_outgoing_to_wire_fn::<SM>(current_authority_set, gossip_engine, keystore, outgoing_rx, channel_type.clone());

			// For taking raw inbound signed messages, mapping them to unsigned messages, then sending
			// to the appropriate AsyncProtocol
			let inbound_signed_message_receiver = Self::generate_inbound_signed_message_receiver_fn::<SM>(params.signed_message_receiver, params.client, params.latest_header, best_authorities, authority_public_key, channel_type.clone(), incoming_tx_proto);

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				select! {
					proto_res = async_proto.fuse() => {
						log::error!(target: "dkg", "🕸️  Protocol {:?} Ended: {:?}", channel_type, proto_res);
						proto_res
					},

					outgoing_res = outgoing_to_wire.fuse() => {
						log::error!(target: "dkg", "🕸️  Outbound Sender Ended: {:?}", outgoing_res);
						outgoing_res
					},

					incoming_res = inbound_signed_message_receiver.fuse() => {
						log::error!(target: "dkg", "🕸️  Inbound Receiver Ended: {:?}", incoming_res);
						incoming_res
					}
				}
			};


			Ok(Self {
				protocol: Box::pin(protocol),
				_pd: Default::default()
			})
		}

		fn new_voting(params: AsyncProtocolParameters<B, C>) -> Result<Self, DKGError> {
			let protocol = Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(params.signed_message_receiver, ProtocolType::Voting);

				// the first step is to generate the partial sig based on the offline stage
				let completed_offline_stage= params.completed_offline_stage.lock().take().ok_or(DKGError::Vote { reason: "Offline stage has not yet been completed".to_string() })?;

				// TODO: determine number of parties
				let number_of_parties = 0;

				let (signing, partial_signature) = SignManual::new(
					// TODO: determine "data to sign"
					BigInt::from_bytes(args.data_to_sign.as_bytes()),
					completed_offline_stage,
				)?;

				let partial_sig_bytes = serde_json::to_vec(&partial_signature).unwrap();

				let party_ind = find_index::<AuthorityId>(&params.best_authorities, &params.authority_public_key).unwrap() as u16 + 1;
				let round_id = params.current_validator_set.read().clone().id;
				let id = params.keystore
					.authority_id(&keystore.public_keys().unwrap())
					.unwrap_or_else(|| panic!("Halp"));

				let payload = DKGMsgPayload::Vote(DKGVoteMessage {
					party_ind,
					round_key: vec![],
					partial_signature: partial_sig_bytes
				});

				// now, broadcast the data
				let unsigned_dkg_message = DKGMessage { id, payload, round_id };
				sign_and_send_messages(&params.gossip_engine,&params.keystore, unsigned_dkg_message);


				// now, take number_of_parties -1 message (TODO: Map raw json-serded bytes to signatures)
				let partial_sigs = incoming_wrapper.take(number_of_parties.saturating_sub(1) as _).map_ok(|r| r.body).try_collect().await?;
				let signature = signing
					.complete(&partial_signatures)
					.context("voting stage failed")?;
				let signature = serde_json::to_string(&signature).context("serialize signature")?;

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			});

			Ok(Self {
				protocol,
				_pd: Default::default()
			})
		}

		fn generate_verification_function(client: Arc<C>, latest_header: Arc<RwLock<Option<B::Header>>>, best_authorities: Vec<Public>, authority_public_key: Public) -> Box<dyn VerifyFn<Arc<SignedDKGMessage<Public>>>> {
			let latest_header = latest_header.clone();
			let client = client.clone();

			Box::new(move |msg: Arc<SignedDKGMessage<Public>>| {
				let latest_header = &*latest_header.read();
				let client = &client;

				let party_inx = find_index::<AuthorityId>(&best_authorities, &authority_public_key).unwrap() as u16 + 1;

				DKGWorker::verify_signature_against_authorities_inner(latest_header, client, (&*msg).clone())
					.map(|msg| (msg, party_inx))
			})
		}

		fn generate_outgoing_to_wire_fn<SM: StateMachineIface>(current_authority_set: Arc<RwLock<AuthoritySet<Public>>>, gossip_engine: Arc<Mutex<GossipEngine<B>>>, keystore: DKGKeystore, mut outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>, proto_ty: ProtocolType) -> Pin<Box<dyn SendFuture>>
			where <SM as StateMachine>::MessageBody: Serialize,
				  <SM as StateMachine>::MessageBody: Send {
			 Box::pin(async move {
				// take all unsigned messages, then sign them and send outbound
				while let Some(unsigned_message) = outgoing_rx.next().await {
					let serialized_body = serde_json::to_vec(&unsigned_message).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
					let round_id = current_authority_set.read().clone().id;
					let id = keystore
						.authority_id(&keystore.public_keys().unwrap())
						.unwrap_or_else(|| panic!("Halp"));

					let payload = match &proto_ty {
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
					sign_and_send_messages(&gossip_engine,&keystore, unsigned_dkg_message);
				}

				Err(DKGError::CriticalError { reason: "Outbound stream stopped producing items".to_string() })
			})
		}

		fn generate_inbound_signed_message_receiver_fn<SM: StateMachineIface>(signed_message_receiver: SignedMessageReceiver,
																			  client: Arc<C>,
																			  latest_header: Arc<RwLock<Option<B::Header>>>,
																			  best_authorities: Vec<Public>,
																			  authority_public_key: Public,
																			  channel_type: ProtocolType,
																			  to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>) -> Pin<Box<dyn SendFuture>>
			where <SM as StateMachine>::MessageBody: Send {
			Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let verify_fn = Self::generate_verification_function(client, latest_header, best_authorities, authority_public_key);
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(signed_message_receiver, channel_type, verify_fn);

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					if let Err(_) = SM::handle_unsigned_message(&to_async_proto, unsigned_message) {
						log::error!(target: "dkg", "Error handling unsigned inbound message. Returning");
						break;
					}
				}

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			})
		}
	}

	impl<B, BE, C> Unpin for MetaDKGMessageHandler<B, BE, C> {}

	impl<B, BE, C> Future for MetaDKGMessageHandler<B, BE, C> {
		type Output = Result<(), DKGError>;

		fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
			self.protocol.as_mut().poll(cx)
		}
	}
}
