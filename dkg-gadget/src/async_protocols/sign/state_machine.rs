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

use crate::async_protocols::{
	blockchain_interface::BlockchainInterface, state_machine::StateMachineHandler,
	AsyncProtocolParameters, ProtocolType,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload};
use dkg_runtime_primitives::crypto::Public;
use futures::channel::mpsc::UnboundedSender;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{
	OfflineProtocolMessage, OfflineStage,
};
use parking_lot::RwLock;
use round_based::{Msg, StateMachine};
use std::sync::Arc;

#[async_trait]
impl StateMachineHandler for OfflineStage {
	type AdditionalReturnParam = Arc<RwLock<Vec<<Self as StateMachine>::Output>>>;
	type Return = ();

	fn handle_unsigned_message(
		to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>,
		msg: Msg<DKGMessage<Public>>,
		local_ty: &ProtocolType,
	) -> Result<(), <Self as StateMachine>::Err> {
		let DKGMessage { payload, .. } = msg.body;

		// Send the payload to the appropriate AsyncProtocols
		match payload {
			DKGMsgPayload::Offline(msg) => {
				let message: Msg<OfflineProtocolMessage> =
					match serde_json::from_slice(msg.offline_msg.as_slice()) {
						Ok(msg) => msg,
						Err(err) => {
							log::error!("Error deserializing offline message: {:?}", err);
							// Skip this message.
							return Ok(())
						},
					};
				if let Some(recv) = message.receiver.as_ref() {
					if *recv != local_ty.get_i() {
						//log::info!("Skipping passing of message to async proto since not
						// intended for local");
						return Ok(())
					}
				}

				if local_ty.uid().map(|uid| uid != msg.uid).unwrap_or(false) {
					log::warn!(
						"Skipping passing of message to async proto since not having the same uid"
					);
					return Ok(())
				}

				if let Err(err) = to_async_proto.unbounded_send(message) {
					log::error!("Error sending message to async proto: {:?}", err);
					// Skip this message.
					return Ok(())
				}
			},

			err => log::debug!(target: "dkg", "Invalid payload received: {:?}", err),
		}

		Ok(())
	}

	async fn on_finish<BI: BlockchainInterface>(
		offline_stage: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BI>,
		store: Self::AdditionalReturnParam,
	) -> Result<(), DKGError> {
		log::info!(target: "dkg", "Completed offline stage successfully!");
		// store the offline stage output
		store.write().push(offline_stage);
		Ok(())
	}
}
