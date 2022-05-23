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

use crate::meta_async_rounds::{
	blockchain_interface::BlockChainIface,
	meta_handler::{AsyncProtocolParameters, MetaAsyncProtocolHandler},
	BatchKey, PartyIndex, ProtocolType, Threshold,
};
use async_trait::async_trait;
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgPayload, DKGPublicKeyMessage, SignedDKGMessage,
};
use dkg_runtime_primitives::{crypto::Public, UnsignedProposal};
use futures::channel::mpsc::UnboundedSender;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::{Keygen, ProtocolMessage},
	sign::{OfflineProtocolMessage, OfflineStage},
};
use round_based::{containers::StoreErr, Msg, StateMachine};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::broadcast::Receiver;

pub(crate) type StateMachineTxRx<T> = (
	futures::channel::mpsc::UnboundedSender<Msg<T>>,
	futures::channel::mpsc::UnboundedReceiver<Msg<T>>,
);

#[async_trait]
/// Trait for interfacing between the meta handler and the individual state machines
pub trait StateMachineIface: StateMachine + Send
where
	<Self as StateMachine>::Output: Send,
{
	type AdditionalReturnParam: Debug + Send;
	type Return: Debug + Send;

	fn generate_channel() -> StateMachineTxRx<<Self as StateMachine>::MessageBody> {
		futures::channel::mpsc::unbounded()
	}

	fn handle_unsigned_message(
		to_async_proto: &futures::channel::mpsc::UnboundedSender<
			Msg<<Self as StateMachine>::MessageBody>,
		>,
		msg: Msg<DKGMessage<Public>>,
		local_ty: &ProtocolType,
	) -> Result<(), <Self as StateMachine>::Err>;

	async fn on_finish<B: BlockChainIface>(
		_result: <Self as StateMachine>::Output,
		_params: AsyncProtocolParameters<B>,
		_additional_param: Self::AdditionalReturnParam,
	) -> Result<Self::Return, DKGError>;
}

#[async_trait]
impl StateMachineIface for Keygen {
	type AdditionalReturnParam = ();
	type Return = <Self as StateMachine>::Output;
	fn handle_unsigned_message(
		to_async_proto: &UnboundedSender<Msg<ProtocolMessage>>,
		msg: Msg<DKGMessage<Public>>,
		local_ty: &ProtocolType,
	) -> Result<(), <Self as StateMachine>::Err> {
		let DKGMessage { payload, .. } = msg.body;
		// Send the payload to the appropriate AsyncProtocols
		match payload {
			DKGMsgPayload::Keygen(msg) => {
				log::info!(target: "dkg", "Handling Keygen inbound message from id={}", msg.round_id);
				use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Error as Error;
				let message: Msg<ProtocolMessage> =
					serde_json::from_slice(msg.keygen_msg.as_slice())
						.map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;

				if let Some(recv) = message.receiver.as_ref() {
					if *recv != local_ty.get_i() {
						log::info!("Skipping passing of message to async proto since not intended for local");
						return Ok(())
					}
				}
				to_async_proto
					.unbounded_send(message)
					.map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
			},

			err => log::debug!(target: "dkg", "Invalid payload received: {:?}", err),
		}

		Ok(())
	}

	async fn on_finish<BCIface: BlockChainIface>(
		local_key: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BCIface>,
		_: Self::AdditionalReturnParam,
	) -> Result<<Self as StateMachine>::Output, DKGError> {
		log::info!(target: "dkg", "Completed keygen stage successfully!");
		// PublicKeyGossip (we need meta handler to handle this)
		// when keygen finishes, we gossip the signed key to peers.
		// [1] create the message, call the "public key gossip" in
		// public_key_gossip.rs:gossip_public_key [2] store public key locally (public_keys.rs:
		// store_aggregated_public_keys)
		let round_id = MetaAsyncProtocolHandler::<()>::get_party_round_id(&params).1;
		let pub_key_msg = DKGPublicKeyMessage {
			round_id,
			pub_key: local_key.public_key().to_bytes(true).to_vec(),
			signature: vec![],
		};

		params.blockchain_iface.gossip_public_key(pub_key_msg)?;
		params.blockchain_iface.store_public_key(local_key.clone(), round_id)?;

		Ok(local_key)
	}
}

#[async_trait]
impl StateMachineIface for OfflineStage {
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
				use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::Error;
				let message: Msg<OfflineProtocolMessage> =
					serde_json::from_slice(msg.offline_msg.as_slice())
						.map_err(|_err| Error::HandleMessage(StoreErr::NotForMe))?;
				if let Some(recv) = message.receiver.as_ref() {
					if *recv != local_ty.get_i() {
						//log::info!("Skipping passing of message to async proto since not
						// intended for local");
						return Ok(())
					}
				}

				if local_ty.get_unsigned_proposal().unwrap().hash().unwrap() != msg.key.as_slice() {
					//log::info!("Skipping passing of message to async proto since not correct
					// unsigned proposal");
					return Ok(())
				}

				to_async_proto
					.unbounded_send(message)
					.map_err(|_| Error::HandleMessage(StoreErr::NotForMe))?;
			},

			err => log::debug!(target: "dkg", "Invalid payload received: {:?}", err),
		}

		Ok(())
	}

	async fn on_finish<BCIface: BlockChainIface>(
		offline_stage: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BCIface>,
		unsigned_proposal: Self::AdditionalReturnParam,
	) -> Result<(), DKGError> {
		log::info!(target: "dkg", "Completed offline stage successfully!");
		// Take the completed offline stage and immediately execute the corresponding voting
		// stage (this will allow parallelism between offline stages executing across the
		// network)
		//
		// NOTE: we pass the generated offline stage id for the i in voting to keep
		// consistency
		MetaAsyncProtocolHandler::new_voting(
			params,
			offline_stage,
			unsigned_proposal.0,
			unsigned_proposal.1,
			unsigned_proposal.2,
			unsigned_proposal.3,
			unsigned_proposal.4,
		)?
		.await?;
		Ok(())
	}
}
