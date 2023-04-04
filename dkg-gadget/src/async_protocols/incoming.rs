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
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
};
use tokio_stream::wrappers::BroadcastStream;

use crate::debug_logger::DebugLogger;

use super::{blockchain_interface::BlockchainInterface, AsyncProtocolParameters, ProtocolType};

/// Used to filter and transform incoming messages from the DKG worker
pub struct IncomingAsyncProtocolWrapper<
	T,
	BI,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
> {
	pub receiver: BroadcastStream<T>,
	session_id: SessionId,
	engine: Arc<BI>,
	logger: DebugLogger,
	ty: ProtocolType<MaxProposalLength>,
}

impl<
		T: TransformIncoming,
		BI: BlockchainInterface,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	> IncomingAsyncProtocolWrapper<T, BI, MaxProposalLength>
{
	pub fn new(
		receiver: tokio::sync::broadcast::Receiver<T>,
		ty: ProtocolType<MaxProposalLength>,
		params: &AsyncProtocolParameters<BI, MaxAuthorities>,
	) -> Self {
		Self {
			receiver: BroadcastStream::new(receiver),
			session_id: params.session_id,
			engine: params.engine.clone(),
			logger: params.logger.clone(),
			ty,
		}
	}
}

pub trait TransformIncoming: Clone + Send + 'static {
	type IncomingMapped;

	fn transform<
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

impl TransformIncoming for Arc<SignedDKGMessage<Public>> {
	type IncomingMapped = DKGMessage<Public>;
	fn transform<
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
				// only clone if the downstream receiver expects this type
				let sender = self.msg.payload.async_proto_only_get_sender_id().unwrap();
				if sender != stream_type.get_i() {
					if self.msg.session_id == this_session_id {
						verify
							.verify_signature_against_authorities(self)
							.map(|body| Some(Msg { sender, receiver: None, body }))
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

impl<T, BI, MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static> Stream
	for IncomingAsyncProtocolWrapper<T, BI, MaxProposalLength>
where
	T: TransformIncoming,
	BI: BlockchainInterface,
{
	type Item = Msg<T::IncomingMapped>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let Self { receiver, ty, engine, session_id, logger } = &mut *self;
		let mut receiver = Pin::new(receiver);

		loop {
			match futures::ready!(receiver.as_mut().poll_next(cx)) {
				Some(Ok(msg)) => match msg.transform(&**engine, &*ty, *session_id, &*logger) {
					Ok(Some(msg)) => return Poll::Ready(Some(msg)),

					Ok(None) => continue,

					Err(err) => {
						logger.warn(format!(
							"While mapping signed message, received an error: {:?}",
							err
						));
						continue
					},
				},
				Some(Err(err)) => {
					logger.error(format!("Stream RECV error: {:?}", err));
					continue
				},
				None => return Poll::Ready(None),
			}
		}
	}
}
