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

use dkg_primitives::types::{DKGError, DKGMessage, NetworkMsgPayload, SessionId, SignedDKGMessage};
use dkg_runtime_primitives::{associated_block_id_acceptable, crypto::Public, MaxAuthorities};
use futures::Stream;
use round_based::Msg;
use sp_runtime::traits::Get;
use std::{
	marker::PhantomData,
	pin::Pin,
	task::{Context, Poll},
};

use crate::debug_logger::DebugLogger;

use super::{blockchain_interface::BlockchainInterface, AsyncProtocolParameters, ProtocolType};

/// Used to filter and transform incoming messages from the DKG worker
pub struct IncomingAsyncProtocolWrapper<
	T: TransformIncoming,
	BI,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
> {
	stream: IncomingStreamMapped<T::IncomingMapped>,
	logger: DebugLogger,
	_pd: PhantomData<(BI, MaxProposalLength)>,
}

pub type IncomingStreamMapped<T> =
	Pin<Box<dyn Stream<Item = Result<Msg<T>, DKGError>> + Send + 'static>>;

impl<
		T: TransformIncoming,
		BI: BlockchainInterface + 'static,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + Unpin + 'static,
	> IncomingAsyncProtocolWrapper<T, BI, MaxProposalLength>
{
	pub fn new(
		mut receiver: tokio::sync::mpsc::UnboundedReceiver<T>,
		ty: ProtocolType<MaxProposalLength>,
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
	) -> Self {
		let logger = params.logger.clone();

		let stream = async_stream::try_stream! {
			while let Some(msg) = receiver.recv().await {
				match msg.transform(&params.engine, &ty, params.session_id, &params.logger).await {
					Ok(Some(msg)) => yield msg,

					Ok(None) => continue,

					Err(err) => {
						params.logger.warn(format!(
							"While mapping signed message, received an error: {err:?}"
						));
						continue
					},
				}
			}
		};

		Self { stream: Box::pin(stream), logger, _pd: Default::default() }
	}
}

#[async_trait::async_trait]
pub trait TransformIncoming: Clone + Send + 'static {
	type IncomingMapped: Send;

	async fn transform<
		BI: BlockchainInterface,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	>(
		self,
		verify: &BI,
		stream_type: &ProtocolType<MaxProposalLength>,
		this_session_id: SessionId,
		logger: &DebugLogger,
	) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError>
	where
		Self: Sized;
}

#[async_trait::async_trait]
impl TransformIncoming for SignedDKGMessage<Public> {
	type IncomingMapped = DKGMessage<Public>;
	async fn transform<
		BI: BlockchainInterface,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	>(
		self,
		verify: &BI,
		stream_type: &ProtocolType<MaxProposalLength>,
		this_session_id: SessionId,
		logger: &DebugLogger,
	) -> Result<Option<Msg<Self::IncomingMapped>>, DKGError>
	where
		Self: Sized,
	{
		logger.checkpoint_message_raw(self.msg.payload.payload(), "CP-2-incoming");
		match (stream_type, &self.msg.payload) {
			(ProtocolType::Keygen { .. }, NetworkMsgPayload::Keygen(..)) |
			(ProtocolType::Offline { .. }, NetworkMsgPayload::Offline(..)) |
			(ProtocolType::Voting { .. }, NetworkMsgPayload::Vote(..)) => {
				logger.checkpoint_message_raw(self.msg.payload.payload(), "CP-2.1-incoming");
				// only clone if the downstream receiver expects this type
				let associated_block_id = stream_type.get_associated_block_id();
				let sender = self
					.msg
					.payload
					.async_proto_only_get_sender_id()
					.expect("Could not get sender id");
				if sender != stream_type.get_i() {
					logger.checkpoint_message_raw(self.msg.payload.payload(), "CP-2.2-incoming");
					if self.msg.session_id == this_session_id {
						logger
							.checkpoint_message_raw(self.msg.payload.payload(), "CP-2.3-incoming");
						if associated_block_id_acceptable(
							associated_block_id,
							self.msg.associated_block_id,
						) {
							logger.checkpoint_message_raw(
								self.msg.payload.payload(),
								"CP-2.4-incoming",
							);
							let payload = self.msg.payload.payload().clone();
							match verify.verify_signature_against_authorities(self).await {
								Ok(body) => {
									logger.checkpoint_message_raw(
										&payload,
										"CP-2.4-verified-incoming",
									);
									Ok(Some(Msg { sender, receiver: None, body }))
								},
								Err(err) => {
									let err_msg = format!("Unable to verify message: {err:?}");
									logger.error(&err_msg);
									logger.checkpoint_message_raw(&payload, err_msg);
									Err(err)
								},
							}
						} else {
							logger.warn(format!("Will skip passing message to state machine since not for this associated block, msg block {:?} expected block {:?}", self.msg.associated_block_id, associated_block_id));
							Ok(None)
						}
					} else {
						logger.warn(format!("Will skip passing message to state machine since not for this round, msg round {:?} this session {:?}", self.msg.session_id, this_session_id));
						Ok(None)
					}
				} else {
					logger.trace("Will skip passing message to state machine since sender is self");
					Ok(None)
				}
			},

			(_l, _r) => {
				// dkg_logging::warn!("Received message for mixed stage: Local: {:?}, payload:
				// {:?}", l, r);
				Ok(None)
			},
		}
	}
}

impl<
		T: TransformIncoming,
		BI: BlockchainInterface,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	> Unpin for IncomingAsyncProtocolWrapper<T, BI, MaxProposalLength>
{
}

impl<
		T: TransformIncoming,
		BI: BlockchainInterface,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	> Stream for IncomingAsyncProtocolWrapper<T, BI, MaxProposalLength>
{
	type Item = Msg<T::IncomingMapped>;
	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		match futures::ready!(self.as_mut().stream.as_mut().poll_next(cx)) {
			Some(Ok(msg)) => Poll::Ready(Some(msg)),
			Some(Err(err)) => {
				self.logger.error(format!("Error in incoming stream: {err:?}"));
				self.poll_next(cx)
			},
			None => Poll::Ready(None),
		}
	}
}
