use log::{debug, error, info, trace, warn};
use round_based::{IsCritical, Msg, StateMachine};

use sp_runtime::traits::AtLeast32BitUnsigned;

use crate::{types::*, utils::vec_usize_to_u16};

pub use gg_2020::{
	party_i::*,
	state_machine::{keygen::*, sign::*},
};
pub use multi_party_ecdsa::protocols::multi_party_ecdsa::{
	gg_2020,
	gg_2020::state_machine::{keygen as gg20_keygen, sign as gg20_sign, traits::RoundBlame},
};

pub enum OfflineState<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	NotStarted(PreOfflineRounds),
	Started(OfflineRounds<Clock>),
	Finished(Result<CompletedOfflineStage, DKGError>),
}

impl<C> DKGRoundsSM<DKGOfflineMessage, OfflineState<C>, C> for OfflineState<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		match self {
			Self::Started(offline_rounds) => offline_rounds.proceed(at),
			_ => Ok(true),
		}
	}

	fn get_outgoing(&mut self) -> Vec<DKGOfflineMessage> {
		match self {
			Self::Started(offline_rounds) => offline_rounds.get_outgoing(),
			_ => vec![],
		}
	}

	fn handle_incoming(&mut self, data: DKGOfflineMessage, at: C) -> Result<(), DKGError> {
		match self {
			Self::NotStarted(pre_offline_rounds) => pre_offline_rounds.handle_incoming(data, at),
			Self::Started(offline_rounds) => offline_rounds.handle_incoming(data, at),
			_ => Ok(()),
		}
	}

	fn is_finished(&self) -> bool {
		match self {
			Self::Started(offline_rounds) => offline_rounds.is_finished(),
			_ => true,
		}
	}

	fn try_finish(self) -> Result<Self, DKGError> {
		match self {
			Self::Started(ref offline_rounds) =>
				if offline_rounds.is_finished() {
					Ok(self)
				} else {
					Err(DKGError::SMNotFinished)
				},
			_ => Ok(self),
		}
	}
}

/// Pre-offline rounds

pub struct PreOfflineRounds {
	signer_set_id: SignerSetId,
	pub pending_offline_msgs: Vec<DKGOfflineMessage>,
}

impl PreOfflineRounds {
	pub fn new(signer_set_id: SignerSetId) -> Self {
		Self { signer_set_id, pending_offline_msgs: Vec::default() }
	}
}

impl<C> DKGRoundsSM<DKGOfflineMessage, Vec<DKGOfflineMessage>, C> for PreOfflineRounds
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn handle_incoming(&mut self, data: DKGOfflineMessage, _at: C) -> Result<(), DKGError> {
		self.pending_offline_msgs.push(data);
		Ok(())
	}

	fn is_finished(&self) -> bool {
		true
	}

	fn try_finish(self) -> Result<Vec<DKGOfflineMessage>, DKGError> {
		Ok(self.pending_offline_msgs)
	}
}

/// Offline rounds

pub struct OfflineRounds<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	params: SignParams,
	started_at: Clock,
	round_key: Vec<u8>,
	offline_stage: OfflineStage,
}

impl<C> OfflineRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn new(
		params: SignParams,
		started_at: C,
		round_key: Vec<u8>,
		offline_stage: OfflineStage,
	) -> Self {
		Self { params, started_at, round_key, offline_stage }
	}
}

impl<C> DKGRoundsSM<DKGOfflineMessage, CompletedOfflineStage, C> for OfflineRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	/// Proceed to next step

	fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		trace!(target: "dkg", "🕸️  OfflineStage party {} enter proceed", self.params.party_index);

		let offline_stage = &mut self.offline_stage;

		if offline_stage.wants_to_proceed() {
			info!(target: "dkg", "🕸️  OfflineStage party {} wants to proceed", offline_stage.party_ind());
			trace!(target: "dkg", "🕸️  before: {:?}", offline_stage);
			// TODO, handle asynchronously
			match offline_stage.proceed() {
				Ok(_) => {
					trace!(target: "dkg", "🕸️  after: {:?}", offline_stage);
				},
				Err(err) => {
					println!("Error proceeding offline stage {:?}", err);
					match err {
						gg20_sign::Error::ProceedRound(proceed_err) => match proceed_err {
							gg20_sign::rounds::Error::Round1(err_type) =>
								return Err(DKGError::OfflineMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_sign::rounds::Error::Round2Stage4(err_type) =>
								return Err(DKGError::OfflineMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_sign::rounds::Error::Round3(err_type) =>
								return Err(DKGError::OfflineMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_sign::rounds::Error::Round5(err_type) =>
								return Err(DKGError::OfflineMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							gg20_sign::rounds::Error::Round6VerifyProof(err_type) =>
								return Err(DKGError::OfflineMisbehaviour {
									bad_actors: vec_usize_to_u16(err_type.bad_actors),
								}),
							_ =>
								return Err(DKGError::GenericError {
									reason: proceed_err.to_string(),
								}),
						},
						_ => return Err(DKGError::GenericError { reason: err.to_string() }),
					};
				},
			}
		}

		let (_, blame_vec) = offline_stage.round_blame();

		if offline_stage.is_finished() {
			Ok(true)
		} else {
			if at - self.started_at > OFFLINE_TIMEOUT.into() {
				if !blame_vec.is_empty() {
					return Err(DKGError::OfflineTimeout { bad_actors: blame_vec })
				} else {
					// Should never happen
					warn!(target: "dkg", "🕸️  Offline timeout reached, but no missing parties found", );
				}
			}
			Ok(false)
		}
	}

	/// Get outgoing messages

	fn get_outgoing(&mut self) -> Vec<DKGOfflineMessage> {
		let mut messages = vec![];
		trace!(target: "dkg", "🕸️  Getting outgoing offline messages");

		let offline_stage = &mut self.offline_stage;

		if !offline_stage.message_queue().is_empty() {
			trace!(target: "dkg", "🕸️  Outgoing messages, queue len: {}", offline_stage.message_queue().len());

			let signer_set_id = self.params.signer_set_id;

			for m in offline_stage.message_queue().into_iter() {
				trace!(target: "dkg", "🕸️  MPC protocol message {:?}", *m);
				let serialized = serde_json::to_string(&m).unwrap();
				let msg = DKGOfflineMessage {
					key: self.round_key.clone(),
					signer_set_id,
					offline_msg: serialized.into_bytes(),
				};

				messages.push(msg);
			}

			offline_stage.message_queue().clear();
		}

		messages
	}

	/// Handle incoming messages

	fn handle_incoming(&mut self, data: DKGOfflineMessage, at: C) -> Result<(), DKGError> {
		if data.signer_set_id != self.params.signer_set_id {
			return Err(DKGError::GenericError { reason: "Signer set ids do not match".to_string() })
		}

		let offline_stage = &mut self.offline_stage;

		trace!(target: "dkg", "🕸️  Handle incoming offline message");
		if data.offline_msg.is_empty() {
			warn!(target: "dkg", "🕸️  Got empty message");
			return Ok(())
		}
		let msg: Msg<OfflineProtocolMessage> = match serde_json::from_slice(&data.offline_msg) {
			Ok(msg) => msg,
			Err(err) => {
				error!(target: "dkg", "🕸️  Error deserializing msg: {:?}", err);
				return Err(DKGError::GenericError {
					reason: "Error deserializing offline msg".to_string(),
				})
			},
		};

		if Some(offline_stage.party_ind()) != msg.receiver &&
			(msg.receiver.is_some() || msg.sender == offline_stage.party_ind())
		{
			warn!(target: "dkg", "🕸️  Ignore messages sent by self");
			return Ok(())
		}
		trace!(
			target: "dkg", "🕸️  Party {} got message from={}, broadcast={}: {:?}",
			offline_stage.party_ind(),
			msg.sender,
			msg.receiver.is_none(),
			msg.body,
		);
		debug!(target: "dkg", "🕸️  State before incoming message processing: {:?}", offline_stage);
		match offline_stage.handle_incoming(msg.clone()) {
			Ok(()) => (),
			Err(err) if err.is_critical() => {
				error!(target: "dkg", "🕸️  Critical error encountered: {:?}", err);
				return Err(DKGError::CriticalError {
					reason: "Offline critical error encountered".to_string(),
				})
			},
			Err(err) => {
				error!(target: "dkg", "🕸️  Non-critical error encountered: {:?}", err);
			},
		}
		debug!(target: "dkg", "🕸️  State after incoming message processing: {:?}", offline_stage);

		Ok(())
	}

	/// Try finish current Stage

	fn is_finished(&self) -> bool {
		self.offline_stage.is_finished()
	}

	fn try_finish(mut self) -> Result<CompletedOfflineStage, DKGError> {
		info!(target: "dkg", "🕸️  Extracting output for offline stage");
		match self.offline_stage.pick_output() {
			Some(Ok(cos)) => {
				info!(target: "dkg", "🕸️  CompletedOfflineStage is extracted");
				Ok(cos)
			},
			Some(Err(err)) => Err(DKGError::CriticalError { reason: err.to_string() }),
			None => Err(DKGError::GenericError {
				reason: "OfflineStage finished with no result".to_string(),
			}),
		}
	}
}
