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
use crate::{
	async_protocols::{
		blockchain_interface::BlockchainInterface, state_machine::StateMachineHandler,
		AsyncProtocolParameters, BatchKey, GenericAsyncHandler, OfflinePartyId, ProtocolType,
		Threshold,
	},
	debug_logger::DebugLogger,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload};
use dkg_runtime_primitives::{
	crypto::Public, MaxAuthorities, StoredUnsignedProposalBatch, UnsignedProposal,
};
use futures::channel::mpsc::UnboundedSender;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{
	OfflineProtocolMessage, OfflineStage,
};
use round_based::{Msg, StateMachine};

#[async_trait]
impl<BI: BlockchainInterface + 'static> StateMachineHandler<BI> for OfflineStage {
	type AdditionalReturnParam = (
		StoredUnsignedProposalBatch<
			<BI as BlockchainInterface>::BatchId,
			<BI as BlockchainInterface>::MaxProposalLength,
			<BI as BlockchainInterface>::MaxProposalsInBatch,
			<BI as BlockchainInterface>::Clock,
		>,
		OfflinePartyId,
		Threshold,
		BatchKey,
	);
	type Return = ();

	fn handle_unsigned_message(
		to_async_proto: &UnboundedSender<Msg<OfflineProtocolMessage>>,
		msg: Msg<DKGMessage<Public>>,
		local_ty: &ProtocolType<
			<BI as BlockchainInterface>::BatchId,
			<BI as BlockchainInterface>::MaxProposalLength,
			<BI as BlockchainInterface>::MaxProposalsInBatch,
			<BI as BlockchainInterface>::Clock,
		>,
		logger: &DebugLogger,
	) -> Result<(), <Self as StateMachine>::Err> {
		let payload_raw = msg.body.payload.payload().clone();
		logger.checkpoint_message_raw(&payload_raw, "CP-2.6-incoming");
		let DKGMessage { payload, .. } = msg.body;

		// Send the payload to the appropriate AsyncProtocols
		match payload {
			DKGMsgPayload::Offline(msg) => {
				logger.checkpoint_message_raw(&payload_raw, "CP-2.7-incoming");
				let message: Msg<OfflineProtocolMessage> =
					match serde_json::from_slice(msg.offline_msg.as_slice()) {
						Ok(msg) => msg,
						Err(err) => {
							logger.error_signing(format!(
								"Error deserializing offline message: {err:?}"
							));
							// Skip this message.
							return Ok(())
						},
					};
				logger.checkpoint_message_raw(&payload_raw, "CP-2.8-incoming");
				if let Some(recv) = message.receiver.as_ref() {
					if *recv != local_ty.get_i() {
						logger.info_signing("Skipping passing of message to async proto since not intended for local");
						logger.clear_checkpoint_for_message_raw(&payload_raw);
						return Ok(())
					}
				}

				if local_ty
					.get_unsigned_proposal()
					.expect("Unsigned proposal not found")
					.hash()
					.expect("Unsigned proposal hash failed") !=
					msg.key.as_slice()
				{
					logger.warn_signing("Skipping passing of message to async proto since not correct unsigned proposal");
					logger.clear_checkpoint_for_message_raw(&payload_raw);
					return Ok(())
				}

				if let Err(err) = to_async_proto.unbounded_send(message) {
					logger.error_signing(format!("Error sending message to async proto: {err}"));
				} else {
					logger.checkpoint_message_raw(&payload_raw, "CP-2.9-incoming");
				}
			},

			err => logger.debug_signing(format!("Invalid payload received: {err:?}")),
		}

		Ok(())
	}

	async fn on_finish(
		offline_stage: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		unsigned_proposal: Self::AdditionalReturnParam,
	) -> Result<(), DKGError> {
		params.logger.info_signing("Completed offline stage successfully!");
		// Take the completed offline stage and immediately execute the corresponding voting
		// stage (this will allow parallelism between offline stages executing across the
		// network)
		//
		// NOTE: we pass the generated offline stage id for the i in voting to keep
		// consistency
		let rx_handle = params.handle.rx_voting.lock().take().expect("rx_voting not found");
		let logger = params.logger.clone();
		match GenericAsyncHandler::new_voting(
			params,
			offline_stage,
			unsigned_proposal.0,
			unsigned_proposal.1,
			rx_handle,
			unsigned_proposal.2,
			unsigned_proposal.3,
		) {
			Ok(voting_stage) => {
				logger.info_signing("Starting voting stage...".to_string());
				if let Err(e) = voting_stage.await {
					logger.error_signing(format!("Error starting voting stage: {e:?}"));
					return Err(e)
				}
				Ok(())
			},
			Err(err) => {
				logger.error_signing(format!("Error starting voting stage: {err:?}"));
				Err(err)
			},
		}
	}
}
