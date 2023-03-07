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

use dkg_primitives::types::SessionId;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::traits::RoundBlame;
use round_based::{Msg, StateMachine};
use sp_runtime::traits::Get;
use std::sync::Arc;

use super::{CurrentRoundBlame, ProtocolType};

pub(crate) struct StateMachineWrapper<T: StateMachine, MaxProposalLength: Get<u32>> {
	sm: T,
	session_id: SessionId,
	channel_type: ProtocolType<MaxProposalLength>,
	current_round_blame: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
}

impl<T: StateMachine + RoundBlame, MaxProposalLength: Get<u32>>
	StateMachineWrapper<T, MaxProposalLength>
{
	pub fn new(
		sm: T,
		session_id: SessionId,
		channel_type: ProtocolType<MaxProposalLength>,
		current_round_blame: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
	) -> Self {
		Self { sm, session_id, channel_type, current_round_blame }
	}

	fn collect_round_blame(&self) {
		let (unreceived_messages, blamed_parties) = self.round_blame();
		let _ = self
			.current_round_blame
			.send(CurrentRoundBlame { unreceived_messages, blamed_parties });
	}
}

impl<T, MaxProposalLength: Get<u32>> StateMachine for StateMachineWrapper<T, MaxProposalLength>
where
	T: StateMachine + RoundBlame,
{
	type Err = T::Err;
	type Output = T::Output;
	type MessageBody = T::MessageBody;

	fn handle_incoming(&mut self, msg: Msg<Self::MessageBody>) -> Result<(), Self::Err> {
		dkg_logging::trace!(
			"Handling incoming message for {:?} from session={}, round={}, sender={}",
			self.channel_type,
			self.session_id,
			self.current_round(),
			msg.sender
		);
		let result = self.sm.handle_incoming(msg);
		self.collect_round_blame();
		result
	}

	fn message_queue(&mut self) -> &mut Vec<Msg<Self::MessageBody>> {
		if !self.sm.message_queue().is_empty() {
			dkg_logging::trace!(
				"Preparing to drain message queue for {:?} in session={}, round={}, queue size={}",
				self.channel_type,
				self.session_id,
				self.current_round(),
				self.sm.message_queue().len(),
			);
		}
		self.sm.message_queue()
	}

	fn wants_to_proceed(&self) -> bool {
		self.sm.wants_to_proceed()
	}

	fn proceed(&mut self) -> Result<(), Self::Err> {
		dkg_logging::trace!(
			"Trying to proceed: current round ({:?}), waiting for msgs from parties: ({:?})",
			self.current_round(),
			self.round_blame(),
		);
		let result = self.sm.proceed();
		dkg_logging::trace!(
			"Proceeded through SM: ({:?}), new current round ({:?}), waiting for msgs from parties: ({:?})",
			self.channel_type,
			self.current_round(),
			self.round_blame(),
		);
		self.collect_round_blame();
		result
	}

	fn round_timeout(&self) -> Option<std::time::Duration> {
		self.sm.round_timeout()
	}

	fn round_timeout_reached(&mut self) -> Self::Err {
		self.sm.round_timeout_reached()
	}

	fn is_finished(&self) -> bool {
		self.sm.is_finished()
	}

	fn pick_output(&mut self) -> Option<Result<Self::Output, Self::Err>> {
		self.sm.pick_output()
	}

	fn current_round(&self) -> u16 {
		self.sm.current_round()
	}

	fn total_rounds(&self) -> Option<u16> {
		self.sm.total_rounds()
	}

	fn party_ind(&self) -> u16 {
		self.sm.party_ind()
	}

	fn parties(&self) -> u16 {
		self.sm.parties()
	}
}

impl<T: StateMachine + RoundBlame, MaxProposalLength: Get<u32>> RoundBlame
	for StateMachineWrapper<T, MaxProposalLength>
{
	fn round_blame(&self) -> (u16, Vec<u16>) {
		self.sm.round_blame()
	}
}
