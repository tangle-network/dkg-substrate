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
use std::fmt::{Debug, Formatter};
use std::sync::Arc;


use curv::elliptic::curves::Secp256k1;
use dkg_primitives::types::{DKGError, DKGMessage, SignedDKGMessage, DKGMsgPayload};

use futures::{stream::Stream};

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{LocalKey, ProtocolMessage};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{CompletedOfflineStage, OfflineProtocolMessage};
use round_based::Msg;


use tokio_stream::wrappers::BroadcastStream;
use dkg_runtime_primitives::crypto::Public;
use dkg_runtime_primitives::UnsignedProposal;

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

#[derive(Clone)]
pub enum ProtocolType {
	Keygen { i: u16, t: u16, n: u16 },
	Offline { unsigned_proposal: UnsignedProposal, i: u16, s_l: Vec<u16>, local_key: LocalKey<Secp256k1> },
	Voting { offline_stage: CompletedOfflineStage, unsigned_proposal: UnsignedProposal }
}

impl Debug for ProtocolType {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			ProtocolType::Keygen { i, t, n } => {
				write!(f, "Keygen: (i, t, n) = ({}, {}, {})", i, t, n)
			}
			ProtocolType::Offline { i, unsigned_proposal, .. } => {
				write!(f, "Offline: (i, proposal) = ({}, {:?})", i, &unsigned_proposal.proposal)
			}
			ProtocolType::Voting { unsigned_proposal, .. } => {
				write!(f, "Voting: proposal = {:?}", &unsigned_proposal.proposal)
			}
		}
	}
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
				(ProtocolType::Voting { .. }, DKGMsgPayload::Vote(..)) => {
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
	use parking_lot::{Mutex};
	use futures::StreamExt;
	use log::debug;
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{Keygen, ProtocolMessage};
	use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{CompletedOfflineStage, OfflineProtocolMessage, OfflineStage, PartialSignature, SignManual};
	use round_based::async_runtime::watcher::StderrWatcher;
	use round_based::{AsyncProtocol, Msg, StateMachine};
	use round_based::containers::StoreErr;
	use sc_client_api::Backend;
	use sc_network_gossip::GossipEngine;
	use serde::Serialize;
	use dkg_runtime_primitives::{AuthoritySetId, DKGApi, keccak_256, Proposal, UnsignedProposal};
	use sp_runtime::traits::{Block, Header};
	use async_trait::async_trait;

	use dkg_runtime_primitives::crypto::Public;
	use dkg_primitives::types::{DKGError, DKGKeygenMessage, DKGMessage, DKGMsgPayload, DKGOfflineMessage, DKGVoteMessage, SignedDKGMessage};
	use dkg_runtime_primitives::crypto::AuthorityId;
	use crate::async_protocol_handlers::{IncomingAsyncProtocolWrapper, ProtocolType, VerifyFn};
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

	#[async_trait]
	trait StateMachineIface: StateMachine + Send + 'static
		where <Self as StateMachine>::Output: Send + 'static {
		type AdditionalReturnParam: Debug + Send + 'static;

		fn generate_channel() -> (futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, futures::channel::mpsc::UnboundedReceiver<Msg<<Self as StateMachine>::MessageBody>>) {
			futures::channel::mpsc::unbounded()
		}

		fn handle_unsigned_message(to_async_proto: &futures::channel::mpsc::UnboundedSender<Msg<<Self as StateMachine>::MessageBody>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err>;

		async fn on_finish<B, BE, C>(_result: <Self as StateMachine>::Output, _params: AsyncProtocolParameters<B, C>, _additional_param: Self::AdditionalReturnParam) -> Result<(), DKGError>
			where 	B: Block,
					 BE: Backend<B>,
					 C: Client<B, BE> + 'static,
					 C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number> {
			Ok(())
		}
	}

	impl StateMachineIface for Keygen {
		type AdditionalReturnParam = ();
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

	#[async_trait]
	impl StateMachineIface for OfflineStage {
		type AdditionalReturnParam = UnsignedProposal;

		fn handle_unsigned_message(to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>, msg: Msg<DKGMessage<Public>>) -> Result<(), <Self as StateMachine>::Err> {
			let DKGMessage { payload, .. } = msg.body;

			// Send the payload to the appropriate AsyncProtocols
			match payload {
				DKGMsgPayload::Offline(msg) => {
					use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::Error as Error;
					let message: Msg<OfflineProtocolMessage> = serde_json::from_slice(msg.offline_msg.as_slice()).map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;
					to_async_proto.unbounded_send(message).map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
				},

				err => debug!(target: "dkg", "Invalid payload received: {:?}", err)
			}

			Ok(())
		}

		async fn on_finish<B, BE, C>(offline_stage: <Self as StateMachine>::Output, params: AsyncProtocolParameters<B, C>, unsigned_proposal: Self::AdditionalReturnParam)
			-> Result<(), DKGError>
				where B: Block,
					  BE: Backend<B>,
					  C: Client<B, BE> + 'static,
					  C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>
		{
			log::info!(target: "dkg", "Completed offline stage successfully!");
			// take the completed offline stage, and, immediately execute the corresponding voting stage
			// (this will allow parallelism between offline stages executing across the network)
			MetaDKGMessageHandler::new(params, ProtocolType::Voting {
				offline_stage,
				unsigned_proposal
			})?.await
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
					Self::new_inner((), Keygen::new(i, t, n).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, params, channel_type)
				}
				ProtocolType::Offline { unsigned_proposal, i, s_l, local_key } => {
					Self::new_inner(unsigned_proposal, OfflineStage::new(i, s_l, local_key).map_err(|err| DKGError::CriticalError { reason: err.to_string() })?, params, channel_type)
				}
				ProtocolType::Voting { offline_stage, unsigned_proposal } => {
					Self::new_voting(params, offline_stage, unsigned_proposal)
				}
			}
		}

		fn new_inner<SM: StateMachineIface>(additional_param: SM::AdditionalReturnParam, sm: SM, params: AsyncProtocolParameters<B, C>, channel_type: ProtocolType) -> Result<Self, DKGError>
			where <SM as StateMachine>::Err: Send + Debug,
				  <SM as StateMachine>::MessageBody: Send,
				  <SM as StateMachine>::MessageBody: Serialize,
				  <SM as StateMachine>::Output: Send + 'static {

			let (incoming_tx_proto, incoming_rx_proto) = SM::generate_channel();
			let (outgoing_tx, outgoing_rx) = futures::channel::mpsc::unbounded();

			let mut async_proto = AsyncProtocol::new(sm, incoming_rx_proto.map(Ok::<_, <SM as StateMachine>::Err>), outgoing_tx.clone())
				.set_watcher(StderrWatcher);

			let params_for_end_of_proto = params.clone();

			let async_proto = Box::pin(async move {
				let res = async_proto.run().await
					.map_err(|err| DKGError::GenericError { reason: format!("{:?}", err) })?;

				SM::on_finish(res, params_for_end_of_proto, additional_param).await
			});

			// For taking all unsigned messages generated by the AsyncProtocols, signing them,
			// and thereafter sending them outbound
			let outgoing_to_wire = Self::generate_outgoing_to_wire_fn::<SM>(params.clone(), outgoing_rx, channel_type.clone());

			// For taking raw inbound signed messages, mapping them to unsigned messages, then sending
			// to the appropriate AsyncProtocol
			let inbound_signed_message_receiver = Self::generate_inbound_signed_message_receiver_fn::<SM>(params,channel_type.clone(), incoming_tx_proto);

			// Combine all futures into a concurrent select subroutine
			let protocol = async move {
				tokio::select! {
					proto_res = async_proto => {
						log::info!(target: "dkg", "🕸️  Protocol {:?} Ended: {:?}", channel_type, proto_res);
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
				protocol: Box::pin(protocol),
				_pd: Default::default()
			})
		}

		fn new_voting(params: AsyncProtocolParameters<B, C>, completed_offline_stage: CompletedOfflineStage, unsigned_proposal: UnsignedProposal) -> Result<Self, DKGError> {
			let protocol = Box::pin(async move {
				let ty = ProtocolType::Voting {
					offline_stage: completed_offline_stage.clone(),
					unsigned_proposal: unsigned_proposal.clone()
				};
				// the below wrapper will map signed messages into unsigned messages
				let verify_fn = Self::generate_verification_function(params.clone());

				let incoming = params.signed_message_receiver.lock().take().unwrap();
				let ref mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(incoming, ty, verify_fn);

				// the first step is to generate the partial sig based on the offline stage
				let number_of_parties = params.best_authorities.len();
				let (party_ind, round_id, id) = Self::get_party_round_id(&params);
				let party_ind = party_ind.unwrap();

				log::info!(target: "dkg", "Will now begin the voting stage with n={} parties for idx={}", number_of_parties, party_ind);

				let message = || -> Result<[u8; 32], DKGError> {
					/*let lock = params.latest_header.read();
					let latest_header = lock.as_ref().ok_or_else(|| DKGError::Vote { reason: "Latest header does not exist".to_string() })?;
					let at: BlockId<B> = BlockId::hash(latest_header.hash());
					let mut unsigned_proposals: Vec<UnsignedProposal> = params.client.runtime_api().get_unsigned_proposals(&at).map_err(|_err| DKGError::Vote { reason: "Unable to obtain unsigned proposals".to_string() })?;
					log::debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

					// get the unsigned proposal for THIS voting stage, which is at the ith index
					let unsigned_proposal = unsigned_proposals.remove(party_ind as _);*/

					let key = (unsigned_proposal.typed_chain_id, unsigned_proposal.key);
					log::debug!(target: "dkg", "Got unsigned proposal with key = {:?}", &key);
					if let Proposal::Unsigned { data, .. } = unsigned_proposal.proposal {
						log::debug!(target: "dkg", "Adding unsigned proposal to hash vec");
						Ok(keccak_256(&data))
					} else {
						Err(DKGError::Vote { reason: "The unsigned proposal for this stage is invalid".to_string() })
					}
				};

				let hash_of_proposal = message()?;

				let (signing, partial_signature) = SignManual::new(
					BigInt::from_bytes(&hash_of_proposal),
					completed_offline_stage,
				).map_err(|err| DKGError::Vote { reason: err.to_string() })?;

				let partial_sig_bytes = serde_json::to_vec(&partial_signature).unwrap();

				let payload = DKGMsgPayload::Vote(DKGVoteMessage {
					party_ind,
					// use the hash of proposal as "round key" ONLY for purposes of ensuring uniqueness
					// We only want voting to happen amongst voters under the SAME proposal, not different proposals
					// This is now especially necessary since we are allowing for parallelism now
					round_key: Vec::from(&hash_of_proposal as &[u8]),
					partial_signature: partial_sig_bytes
				});

				// now, broadcast the data
				let unsigned_dkg_message = DKGMessage { id, payload, round_id };
				sign_and_send_messages(&params.gossip_engine,&params.keystore, unsigned_dkg_message);

				let number_of_partial_sigs = number_of_parties.saturating_sub(1) as usize;
				let mut sigs = Vec::with_capacity(number_of_partial_sigs);

				// obtain number of parties - 1 messages (i.e., all except self)
				while let Some(msg) = incoming_wrapper.take(number_of_partial_sigs).next().await {
					match msg.body.payload {
						DKGMsgPayload::Vote(dkg_vote_msg) => {
							// only process this message
							if dkg_vote_msg.round_key.as_slice() == hash_of_proposal {
								log::info!(target: "dkg", "Found matching round key!");
								let partial = serde_json::from_slice::<PartialSignature>(&dkg_vote_msg.partial_signature).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
								sigs.push(partial);
							} else {
								log::info!(target: "dkg", "Skipping DKG vote message since round keys did not match");
							}
						}

						_ => unreachable!("Should not happen")
					}
				}

				if sigs.len() != number_of_partial_sigs {
					log::error!(target: "dkg", "Received number of signs not equal to expected (received: {} | expected: {})", sigs.len(), number_of_partial_sigs);
					return Err(DKGError::Vote { reason: "Invalid number of received partial sigs".to_string() })
				}


				let signature = signing
					.complete(&sigs)
					.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;

				let _signature = serde_json::to_string(&signature).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
				// TODO: determine what to do

				Err::<(), _>(DKGError::CriticalError { reason: "Inbound stream stopped producing items".to_string() })
			});

			Ok(Self {
				protocol,
				_pd: Default::default()
			})
		}

		fn get_party_round_id(params: &AsyncProtocolParameters<B, C>) -> (Option<u16>, AuthoritySetId, Public) {
			let party_ind = find_index::<AuthorityId>(&params.best_authorities, &params.authority_public_key).map(|r| r as u16 + 1);
			let round_id = params.current_validator_set.read().clone().id;
			let id = params.keystore
				.authority_id(&params.keystore.public_keys().unwrap())
				.unwrap_or_else(|| panic!("Halp"));

			(party_ind, round_id, id)
		}

		fn generate_verification_function(params: AsyncProtocolParameters<B, C>) -> Box<dyn VerifyFn<Arc<SignedDKGMessage<Public>>>> {
			let latest_header = params.latest_header;
			let client = params.client;

			Box::new(move |msg: Arc<SignedDKGMessage<Public>>| {
				let latest_header = &*latest_header.read();
				let client = &client;

				let party_inx = find_index::<AuthorityId>(&params.best_authorities, &params.authority_public_key).unwrap() as u16 + 1;

				DKGWorker::verify_signature_against_authorities_inner(latest_header, client, (&*msg).clone())
					.map(|msg| (msg, party_inx))
			})
		}

		fn generate_outgoing_to_wire_fn<SM: StateMachineIface>(params: AsyncProtocolParameters<B, C>, mut outgoing_rx: UnboundedReceiver<Msg<<SM as StateMachine>::MessageBody>>, proto_ty: ProtocolType) -> Pin<Box<dyn SendFuture>>
			where <SM as StateMachine>::MessageBody: Serialize,
				  <SM as StateMachine>::MessageBody: Send,
				  <SM as StateMachine>::Output: Send + 'static {
			 Box::pin(async move {
				// take all unsigned messages, then sign them and send outbound
				while let Some(unsigned_message) = outgoing_rx.next().await {
					let serialized_body = serde_json::to_vec(&unsigned_message).map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
					let (_, round_id, id) = Self::get_party_round_id(&params);

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
					sign_and_send_messages(&params.gossip_engine,&params.keystore, unsigned_dkg_message);
				}

				Err(DKGError::CriticalError { reason: "Outbound stream stopped producing items".to_string() })
			})
		}

		fn generate_inbound_signed_message_receiver_fn<SM: StateMachineIface>(params: AsyncProtocolParameters<B, C>,
																			  channel_type: ProtocolType,
																			  to_async_proto: UnboundedSender<Msg<<SM as StateMachine>::MessageBody>>) -> Pin<Box<dyn SendFuture>>
			where <SM as StateMachine>::MessageBody: Send,
				  <SM as StateMachine>::Output: Send + 'static {
			Box::pin(async move {
				// the below wrapper will map signed messages into unsigned messages
				let incoming = params.signed_message_receiver.lock().take().unwrap();
				let verify_fn = Self::generate_verification_function(params);
				let mut incoming_wrapper = IncomingAsyncProtocolWrapper::new(incoming, channel_type, verify_fn);

				while let Some(unsigned_message) = incoming_wrapper.next().await {
					// TODO: we ensure the payload type, but, not that the message is for the nth index
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
