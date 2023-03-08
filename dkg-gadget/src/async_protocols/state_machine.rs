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

use super::{blockchain_interface::BlockchainInterface, AsyncProtocolParameters, ProtocolType};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, DKGMessage};
use dkg_runtime_primitives::crypto::Public;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::traits::RoundBlame;
use round_based::{Msg, StateMachine};
use sp_runtime::traits::Get;
use std::fmt::Debug;

pub(crate) type StateMachineTxRx<T> = (
	futures::channel::mpsc::UnboundedSender<Msg<T>>,
	futures::channel::mpsc::UnboundedReceiver<Msg<T>>,
);

#[async_trait]
/// Trait for interfacing between the meta handler and the individual state machines
pub trait StateMachineHandler<BI: BlockchainInterface + 'static>:
	StateMachine + RoundBlame + Send
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
		local_ty: &ProtocolType<<BI as BlockchainInterface>::MaxProposalLength>,
	) -> Result<(), <Self as StateMachine>::Err>;

	async fn on_finish(
		result: <Self as StateMachine>::Output,
		params: AsyncProtocolParameters<BI>,
		additional_param: Self::AdditionalReturnParam,
		async_index: u8,
	) -> Result<Self::Return, DKGError>;
}
