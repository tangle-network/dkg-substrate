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
	AsyncProtocolParameters, BatchKey, GenericAsyncHandler, PartyIndex, ProtocolType, Threshold,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload, SignedDKGMessage};
use dkg_runtime_primitives::{crypto::Public, UnsignedProposal};
use futures::channel::mpsc::UnboundedSender;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{
	OfflineProtocolMessage, OfflineStage,
};
use round_based::{Msg, StateMachine};
use std::sync::Arc;
use tokio::sync::broadcast::Receiver;

#[async_trait]
impl StateMachineHandler for OfflineStage {
	type AdditionalReturnParam = (
		UnsignedProposal,
		PartyIndex,
		Receiver<Arc<SignedDKGMessage<Public>>>,
		Threshold,
		BatchKey,
	);
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
							dkg_logging::error!("Error deserializing offline message: {:?}", err);
							// Skip this message.
							return Ok(())
						},
					};
				if let Some(recv) = message.receiver.as_ref() {
					if *recv != local_ty.get_i() {
						//dkg_logging::info!("Skipping passing of message to async proto since not
						// intended for local");
						return Ok(())
					}
				}

				if local_ty.get_unsigned_proposal().unwrap().hash().unwrap() != msg.key.as_slice() {
					//dkg_logging::info!("Skipping passing of message to async proto since not
					// correct unsigned proposal");
					return Ok(())
				}

				if let Err(err) = to_async_proto.unbounded_send(message) {
					dkg_logging::error!(target: "dkg_gadget::async_protocol::sign", "Error sending message to async proto: {}", err);
				}
			},

			err => dkg_logging::debug!(target: "dkg_gadget", "Invalid payload received: {:?}", err),
		}

		Ok(())
	}

	async fn on_finish<BI: BlockchainInterface + 'static>(
		offline_stage: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BI>,
		unsigned_proposal: Self::AdditionalReturnParam,
		async_index: u8,
	) -> Result<(), DKGError> {
		dkg_logging::info!(target: "dkg_gadget", "Completed offline stage successfully!");
		// Take the completed offline stage and immediately execute the corresponding voting
		// stage (this will allow parallelism between offline stages executing across the
		// network)
		//
		// NOTE: we pass the generated offline stage id for the i in voting to keep
		// consistency
		match GenericAsyncHandler::new_voting(
			params,
			offline_stage,
			unsigned_proposal.0,
			unsigned_proposal.1,
			unsigned_proposal.2,
			unsigned_proposal.3,
			unsigned_proposal.4,
			async_index,
		) {
			Ok(voting_stage) => {
				dkg_logging::info!(target: "dkg_gadget", "Starting voting stage...");
				if let Err(e) = voting_stage.await {
					dkg_logging::error!(target: "dkg_gadget", "Error starting voting stage: {:?}", e);
					return Err(e)
				}
				Ok(())
			},
			Err(err) => {
				dkg_logging::error!(target: "dkg_gadget", "Error starting voting stage: {:?}", err);
				Err(err)
			},
		}
	}
}
