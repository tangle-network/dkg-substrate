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

use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload, SessionId, SignedDKGMessage};
use dkg_runtime_primitives::{crypto::Public, MaxAuthorities};
use futures::Stream;
use round_based::Msg;
use sp_runtime::traits::Get;
use std::{
	marker::PhantomData,
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
};

use dkg_logging::*;

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
		mut receiver: tokio::sync::broadcast::Receiver<T>,
		ty: ProtocolType<MaxProposalLength>,
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
	) -> Self {
		let logger = params.logger.clone();

		let stream = async_stream::try_stream! {
			while let Ok(msg) = receiver.recv().await {
				params.logger.checkpoint_raw(&ty, msg.payload_message(), "CP5-incoming", false);
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

	fn payload_message(&self) -> Vec<u8>;

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
impl TransformIncoming for Arc<SignedDKGMessage<Public>> {
	type IncomingMapped = DKGMessage<Public>;

	fn payload_message(&self) -> Vec<u8> {
		self.msg.payload.payload_message()
	}

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
		match (stream_type, &self.msg.payload) {
			(ProtocolType::Keygen { .. }, DKGMsgPayload::Keygen(..)) |
			(ProtocolType::Offline { .. }, DKGMsgPayload::Offline(..)) |
			(ProtocolType::Voting { .. }, DKGMsgPayload::Vote(..)) => {
				let payload_message = self.payload_message();
				logger.checkpoint_raw(
					stream_type,
					payload_message.clone(),
					"CP5-incoming-2",
					false,
				);
				// only clone if the downstream receiver expects this type
				let sender = self
					.msg
					.payload
					.async_proto_only_get_sender_id()
					.expect("Could not get sender id");
				logger.checkpoint_raw(
					stream_type,
					payload_message.clone(),
					"CP5-incoming-3",
					false,
				);
				if self.msg.session_id == this_session_id {
					logger.checkpoint_raw(
						stream_type,
						payload_message.clone(),
						"CP5-incoming-4",
						false,
					);
					if let Ok(Some(msg)) = verify
						.verify_signature_against_authorities(self)
						.await
						.map(|body| Some(Msg { sender, receiver: None, body }))
					{
						logger.checkpoint_raw(
							stream_type,
							payload_message.clone(),
							"CP5-incoming-5",
							false,
						);
						Ok(Some(msg))
					} else {
						logger.warn("Could not verify signature of message");
						Ok(None)
					}
				} else {
					logger.warn(format!("Will skip passing message to state machine since not for this round, msg round {:?} this session {:?}", self.msg.session_id, this_session_id));
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
