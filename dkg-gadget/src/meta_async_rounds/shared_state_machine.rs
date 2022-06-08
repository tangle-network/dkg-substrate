use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::traits::RoundBlame;
use parking_lot::Mutex;
use round_based::{Msg, StateMachine};
use std::sync::Arc;

pub(crate) struct SharedStateMachine<T: StateMachine> {
	sm: T,
	blame_vec: Arc<Mutex<(u16, Vec<u16>)>>,
}

impl<T: StateMachine + RoundBlame> SharedStateMachine<T> {
	fn update(&self) {
		let blame_vec = self.round_blame();
		*self.blame_vec.lock() = blame_vec;
	}
}

impl<T> StateMachine for SharedStateMachine<T>
where
	T: StateMachine + RoundBlame,
{
	type Err = T::Err;
	type Output = T::Output;
	type MessageBody = T::MessageBody;

	fn handle_incoming(&mut self, msg: Msg<Self::MessageBody>) -> Result<(), Self::Err> {
		let result = self.sm.handle_incoming(msg);
		self.update();
		result
	}

	fn message_queue(&mut self) -> &mut Vec<Msg<Self::MessageBody>> {
		self.sm.message_queue()
	}

	fn wants_to_proceed(&self) -> bool {
		self.sm.wants_to_proceed()
	}

	fn proceed(&mut self) -> Result<(), Self::Err> {
		let result = self.sm.proceed();
		self.update();
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

impl<T: StateMachine + RoundBlame> RoundBlame for SharedStateMachine<T> {
	fn round_blame(&self) -> (u16, Vec<u16>) {
		self.sm.round_blame()
	}
}
