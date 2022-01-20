use codec::Encode;
use curv::{arithmetic::Converter, elliptic::curves::Secp256k1, BigInt};
use log::{debug, error, info, trace, warn};
use round_based::{IsCritical, Msg, StateMachine};
use sc_keystore::LocalKeystore;
use sp_core::{ecdsa::Signature, sr25519, Pair as TraitPair};
use sp_runtime::traits::AtLeast32BitUnsigned;
use std::{
	collections::{BTreeMap, HashMap},
	path::PathBuf,
	sync::Arc,
};

use crate::{
	types::*,
	utils::{select_random_set, store_localkey, vec_usize_to_u16},
};
use dkg_runtime_primitives::{
	keccak_256,
	offchain_crypto::{Pair as AppPair, Public},
};

pub use gg_2020::{
	party_i::*,
	state_machine::{keygen::*, sign::*},
};
pub use multi_party_ecdsa::protocols::multi_party_ecdsa::{
	gg_2020,
	gg_2020::state_machine::{keygen as gg20_keygen, sign as gg20_sign, traits::RoundBlame},
};

// Keygen state

pub enum KeygenState<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	Empty,
	NotStarted(PreKeygenRounds),
	Started(KeygenRounds<Clock>),
	Finished(Result<LocalKey<Secp256k1>, DKGError>),
}

impl<C> DKGRoundsSM<DKGKeygenMessage, KeygenState<C>, C> for KeygenState<C> 
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.proceed(at),
			_ => Ok(true)
		}
	}

	fn get_outgoing(&mut self) -> Vec<DKGKeygenMessage> {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.get_outgoing(),
			_ => vec![]
		}
	}

	fn handle_incoming(&mut self, data: DKGKeygenMessage, at: C) -> Result<(), DKGError> {
		match self {
			Self::NotStarted(pre_keygen_rounds) => pre_keygen_rounds.handle_incoming(data, at),
			Self::Started(keygen_rounds) => keygen_rounds.handle_incoming(data, at),
			_ => Ok(())
		}
	}

	fn is_finished(&self) -> bool {
		match self {
			Self::Started(keygen_rounds) => keygen_rounds.is_finished(),
			_ => true
		}
	}

	fn try_finish(self) -> Result<Self, DKGError> {
		match self {
			Self::Started(ref keygen_rounds) => {
				if keygen_rounds.is_finished() {
					Ok(self)
				} else {
					Err(DKGError::SMNotFinished)
				}
			},
			_ => Ok(self)
		}
	}
}

/// Pre-keygen rounds

pub struct PreKeygenRounds {
	round_id: RoundId,
	pub pending_keygen_msgs: Vec<DKGKeygenMessage>,
}

impl PreKeygenRounds {
	pub fn new(round_id: RoundId) -> Self {
		Self{
			round_id,
			pending_keygen_msgs: Vec::default(),
		}
	}
}

impl<C> DKGRoundsSM<DKGKeygenMessage, Vec<DKGKeygenMessage>, C> for PreKeygenRounds
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

pub struct KeygenRounds<Clock> where Clock: AtLeast32BitUnsigned + Copy {
	params: KeygenParams,
	started_at: Clock,
	keygen: Keygen,
}

impl<C> KeygenRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn new(
		params: KeygenParams,
		started_at: C,
		keygen: Keygen
	) -> Self {
		Self {
			params,
			started_at,
			keygen,
		}
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
			info!(target: "dkg", "🕸️  Keygen party {} wants to proceed", keygen.party_ind());
			trace!(target: "dkg", "🕸️  before: {:?}", keygen);

			match keygen.proceed() {
				Ok(_) => {
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

			let keygen_set_id = self.params.keygen_set_id;

			let enc_messages = keygen
				.message_queue()
				.into_iter()
				.map(|m| {
					trace!(target: "dkg", "🕸️  MPC protocol message {:?}", m);
					let serialized = serde_json::to_string(&m).unwrap();
					return DKGKeygenMessage {
						keygen_set_id,
						keygen_msg: serialized.into_bytes(),
					}
				})
				.collect::<Vec<DKGKeygenMessage>>();

			keygen.message_queue().clear();
			return enc_messages
		}

		vec![]
	}

	/// Handle incoming messages

	fn handle_incoming(&mut self, data: DKGKeygenMessage, at: C) -> Result<(), DKGError> {
		if data.keygen_set_id != self.params.keygen_set_id {
			return Err(DKGError::GenericError { reason: "Keygen set ids do not match".to_string() })
		}

		let keygen = &mut self.keygen;

		trace!(target: "dkg", "🕸️  Handle incoming keygen message");
		if data.keygen_msg.is_empty() {
			warn!(
				target: "dkg", "🕸️  Got empty message");
			return Ok(())
		}

		let msg: Msg<ProtocolMessage> = match serde_json::from_slice(&data.keygen_msg) {
			Ok(msg) => msg,
			Err(err) => {
				error!(target: "dkg", "🕸️  Error deserializing msg: {:?}", err);
				return Err(DKGError::GenericError {
					reason: "Error deserializing keygen msg".to_string(),
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
		debug!(target: "dkg", "🕸️  State before incoming message processing: {:?}", keygen);
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
		debug!(target: "dkg", "🕸️  State after incoming message processing: {:?}", keygen);
		
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
			Some(Err(e)) => panic!("Keygen finished with error result {}", e),
			None => panic!("Keygen finished with no result"),
		}
	}
}
