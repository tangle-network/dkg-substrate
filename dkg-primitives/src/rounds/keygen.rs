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
//
use curv::elliptic::curves::Secp256k1;
use log::{debug, error, info, trace, warn};
use round_based::{IsCritical, Msg, StateMachine};

use sp_runtime::traits::AtLeast32BitUnsigned;

use crate::{types::*, utils::vec_usize_to_u16};
use std::marker::PhantomData;

pub use gg_2020::{
	party_i::*,
	state_machine::{keygen::*, sign::*},
};
pub use multi_party_ecdsa::protocols::multi_party_ecdsa::{
	gg_2020,
	gg_2020::state_machine::{keygen as gg20_keygen, sign as gg20_sign, traits::RoundBlame},
};

/// Wrapper state-machine for Keygen rounds
pub enum KeygenState<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	Empty,
	NotStarted(PreKeygenRounds<Clock>),
	Started(KeygenRounds<Clock>),
	Finished(Result<LocalKey<Secp256k1>, DKGError>),
}

/// Implementation of DKGRoundsSM trait that dispatches calls
/// to the current internal state if applicable.
impl<C> DKGRoundsSM<DKGKeygenMessage, KeygenState<C>, C> for KeygenState<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.proceed(at),
			_ => Ok(true),
		}
	}

	fn get_outgoing(&mut self) -> Vec<DKGKeygenMessage> {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.get_outgoing(),
			_ => Vec::new(),
		}
	}

	fn handle_incoming(&mut self, data: DKGKeygenMessage, at: C) -> Result<(), DKGError> {
		match self {
			Self::NotStarted(pre_keygen_rounds) => pre_keygen_rounds.handle_incoming(data, at),
			Self::Started(keygen_rounds) => keygen_rounds.handle_incoming(data, at),
			_ => Ok(()),
		}
	}

	fn is_finished(&self) -> bool {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.is_finished(),
			_ => true,
		}
	}

	fn try_finish(self) -> Result<Self, DKGError> {
		match self {
			Self::Started(ref keygen_rounds) =>
				if keygen_rounds.is_finished() {
					Ok(self)
				} else {
					Err(DKGError::SMNotFinished)
				},
			_ => Ok(self),
		}
	}
}

/// Pre-keygen rounds
/// Used to collect incoming messages from other peers which started earlier
pub struct PreKeygenRounds<Clock> {
	pending_keygen_msgs: Vec<DKGKeygenMessage>,
	clock_type: PhantomData<Clock>,
}

impl<C> PreKeygenRounds<C> {
	pub fn new() -> Self {
		Self { pending_keygen_msgs: Vec::default(), clock_type: PhantomData }
	}
}

impl<C> DKGRoundsSM<DKGKeygenMessage, Vec<DKGKeygenMessage>, C> for PreKeygenRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn handle_incoming(&mut self, data: DKGKeygenMessage, _at: C) -> Result<(), DKGError> {
		self.pending_keygen_msgs.push(data);
		Ok(())
	}

	fn is_finished(&self) -> bool {
		true
	}

	fn try_finish(self) -> Result<Vec<DKGKeygenMessage>, DKGError> {
		Ok(self.pending_keygen_msgs)
	}
}

/// Keygen rounds
/// Main state, corresponds to gg20 Keygen state-machine
pub struct KeygenRounds<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	params: KeygenParams,
	started_at: Clock,
	keygen: Keygen,
}

impl<C> KeygenRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn new(params: KeygenParams, started_at: C, keygen: Keygen) -> Self {
		Self { params, started_at, keygen }
	}
}

impl<C> DKGRoundsSM<DKGKeygenMessage, LocalKey<Secp256k1>, C> for KeygenRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	/// Proceed to next step

	fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		trace!(target: "dkg", "🕸️  Keygen party {} enter proceed", self.params.party_index);

		let keygen = &mut self.keygen;

		if keygen.wants_to_proceed() {
			debug!(target: "dkg", "🕸️  Keygen party {} wants to proceed", keygen.party_ind());
			trace!(target: "dkg", "🕸️  before: {:?}", keygen);

			match keygen.proceed() {
				Ok(_) => {
					debug!(target: "dkg", "🕸️  Keygen party {} proceeded", keygen.party_ind());
					trace!(target: "dkg", "🕸️  after: {:?}", keygen);
				},
				Err(err) => {
					match err {
						gg20_keygen::Error::ProceedRound(proceed_err) => match proceed_err {
							gg20_keygen::ProceedError::Round2VerifyCommitments(err_type) =>
								return Err(DKGError::KeygenMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_keygen::ProceedError::Round3VerifyVssConstruct(err_type) =>
								return Err(DKGError::KeygenMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_keygen::ProceedError::Round4VerifyDLogProof(err_type) =>
								return Err(DKGError::KeygenMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
						},
						_ => return Err(DKGError::GenericError { reason: err.to_string() }),
					};
				},
			}
		}

		let (_, blame_vec) = keygen.round_blame();

		if keygen.is_finished() {
			Ok(true)
		} else {
			if at - self.started_at > KEYGEN_TIMEOUT.into() {
				if !blame_vec.is_empty() {
					return Err(DKGError::KeygenTimeout { bad_actors: blame_vec })
				} else {
					// Should never happen
					warn!(target: "dkg", "🕸️  Keygen timeout reached, but no missing parties found", );
				}
			}
			Ok(false)
		}
	}

	/// Get outgoing messages

	fn get_outgoing(&mut self) -> Vec<DKGKeygenMessage> {
		trace!(target: "dkg", "🕸️  Getting outgoing keygen messages");

		let keygen = &mut self.keygen;

		if !keygen.message_queue().is_empty() {
			trace!(target: "dkg", "🕸️  Outgoing messages, queue len: {}", keygen.message_queue().len());

			let round_id = self.params.round_id;

			let enc_messages = keygen
				.message_queue()
				.into_iter()
				.map(|m| {
					trace!(target: "dkg", "🕸️  MPC protocol message {:?}", m);
					let serialized = serde_json::to_string(&m).unwrap();
					return DKGKeygenMessage { round_id, keygen_msg: serialized.into_bytes() }
				})
				.collect::<Vec<DKGKeygenMessage>>();

			keygen.message_queue().clear();
			return enc_messages
		} else {
			Vec::new()
		}
	}

	/// Handle incoming messages

	fn handle_incoming(&mut self, data: DKGKeygenMessage, _at: C) -> Result<(), DKGError> {
		if data.round_id != self.params.round_id {
			return Err(DKGError::GenericError { reason: "Round ids do not match".to_string() })
		}

		let keygen = &mut self.keygen;

		trace!(target: "dkg", "🕸️  Handle incoming keygen message");
		if data.keygen_msg.is_empty() {
			warn!(target: "dkg", "🕸️  Got empty message");
			return Ok(())
		}

		let msg: Msg<ProtocolMessage> = match serde_json::from_slice(&data.keygen_msg) {
			Ok(msg) => msg,
			Err(err) => {
				error!(target: "dkg", "🕸️  Error deserializing msg: {:?}", err);
				return Err(DKGError::GenericError {
					reason: format!("Error deserializing keygen msg, reason: {}", err),
				})
			},
		};

		if Some(keygen.party_ind()) != msg.receiver &&
			(msg.receiver.is_some() || msg.sender == keygen.party_ind())
		{
			warn!(target: "dkg", "🕸️  Ignore messages sent by self");
			return Ok(())
		}
		trace!(
			target: "dkg", "🕸️  Party {} got message from={}, broadcast={}: {:?}",
			keygen.party_ind(),
			msg.sender,
			msg.receiver.is_none(),
			msg.body,
		);
		trace!(target: "dkg", "🕸️  State before incoming message processing: {:?}", keygen);
		match keygen.handle_incoming(msg.clone()) {
			Ok(()) => (),
			Err(err) if err.is_critical() => {
				error!(target: "dkg", "🕸️  Critical error encountered: {:?}", err);
				return Err(DKGError::GenericError {
					reason: "Keygen critical error encountered".to_string(),
				})
			},
			Err(err) => {
				error!(target: "dkg", "🕸️  Non-critical error encountered: {:?}", err);
			},
		}
		trace!(target: "dkg", "🕸️  State after incoming message processing: {:?}", keygen);

		Ok(())
	}

	/// Try finish

	fn is_finished(&self) -> bool {
		self.keygen.is_finished()
	}

	fn try_finish(mut self) -> Result<LocalKey<Secp256k1>, DKGError> {
		info!(target: "dkg", "🕸️  Keygen is finished, extracting output, round_id: {:?}", self.params.round_id);
		match self.keygen.pick_output() {
			Some(Ok(key)) => {
				info!(target: "dkg", "🕸️  local share key is extracted");
				Ok(key)
			},
			Some(Err(err)) => Err(DKGError::CriticalError { reason: err.to_string() }),
			None => Err(DKGError::CriticalError {
				reason: "Keygen finished with no result".to_string(),
			}),
		}
	}
}
