use bincode;
use codec::Encode;
use curv::{
	arithmetic::Converter,
	elliptic::curves::{secp256_k1::Secp256k1Point, traits::ECScalar},
	BigInt,
};
use log::{debug, error, info, trace, warn};
use round_based::{IsCritical, Msg, StateMachine};
use sc_keystore::LocalKeystore;
use sp_core::{ecdsa::Signature, sr25519, Pair as TraitPair};
use sp_runtime::traits::{AtLeast32BitUnsigned, Block, NumberFor};
use std::{
	collections::{BTreeMap, HashMap},
	path::PathBuf,
	sync::Arc,
};

use crate::{
	types::*,
	utils::{store_localkey, vec_usize_to_u16},
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

/// DKG State tracker
pub struct DKGState<K, C> {
	pub accepted: bool,
	pub is_epoch_over: bool,
	pub listening_for_pub_key: bool,
	pub listening_for_active_pub_key: bool,
	pub curr_dkg: Option<MultiPartyECDSARounds<K, C>>,
	pub past_dkg: Option<MultiPartyECDSARounds<K, C>>,
	pub voted_on: HashMap<K, C>,
}

const KEYGEN_TIMEOUT: u32 = 10;
const OFFLINE_TIMEOUT: u32 = 10;
const SIGN_TIMEOUT: u32 = 3;

/// State machine structure for performing Keygen, Offline stage and Sign rounds
pub struct MultiPartyECDSARounds<SignPayloadKey, Clock> {
	round_id: RoundId,
	party_index: u16,
	threshold: u16,
	parties: u16,
	last_received_at: Clock,
	stage_at_last_receipt: Stage,

	keygen_set_id: KeygenSetId,
	signer_set_id: SignerSetId,
	signers: Vec<u16>,
	stage: Stage,

	// DKG clock
	keygen_started_at: Clock,
	offline_started_at: Clock,

	// Message processing
	pending_keygen_msgs: Vec<DKGKeygenMessage>,
	pending_offline_msgs: Vec<DKGOfflineMessage>,

	// Key generation
	keygen: Option<Keygen>,
	local_key: Option<LocalKey>,

	// Offline stage
	offline_stage: Option<OfflineStage>,
	completed_offline_stage: Option<CompletedOfflineStage>,

	// Signing rounds
	rounds: BTreeMap<SignPayloadKey, DKGRoundTracker<Vec<u8>, Clock>>,
	sign_outgoing_msgs: Vec<DKGVoteMessage<SignPayloadKey>>,
	finished_rounds: Vec<DKGSignedPayload<SignPayloadKey>>,

	// File system storage
	local_key_path: Option<PathBuf>,
	public_key: Option<sr25519::Public>,
	local_keystore: Option<Arc<LocalKeystore>>,
}

impl<K, C> MultiPartyECDSARounds<K, C>
where
	K: Ord + Encode + Copy + core::fmt::Debug,
	C: AtLeast32BitUnsigned + Copy,
{
	/// Public ///

	pub fn new(
		party_index: u16,
		threshold: u16,
		parties: u16,
		round_id: RoundId,
		local_key_path: Option<PathBuf>,
		created_at: C,
		public_key: Option<sr25519::Public>,
		local_keystore: Option<Arc<LocalKeystore>>,
	) -> Self {
		trace!(target: "dkg", "🕸️  Creating new MultiPartyECDSARounds, party_index: {}, threshold: {}, parties: {}", party_index, threshold, parties);

		Self {
			party_index,
			threshold,
			parties,
			round_id,
			last_received_at: created_at,
			stage_at_last_receipt: Stage::KeygenReady,
			keygen_set_id: 0,
			signer_set_id: 0,
			signers: Vec::new(),
			keygen_started_at: 0u32.into(),
			offline_started_at: 0u32.into(),
			stage: Stage::KeygenReady,
			pending_keygen_msgs: Vec::new(),
			pending_offline_msgs: Vec::new(),
			keygen: None,
			local_key: None,
			offline_stage: None,
			completed_offline_stage: None,
			rounds: BTreeMap::new(),
			sign_outgoing_msgs: Vec::new(),
			finished_rounds: Vec::new(),
			local_key_path,
			public_key,
			local_keystore,
		}
	}

	pub fn set_local_key(&mut self, local_key: LocalKey) {
		self.local_key = Some(local_key)
	}

	pub fn set_stage(&mut self, stage: Stage) {
		self.stage = stage;
	}

	// We check if the protocol has stalled at a particular stage, if messages are no longer
	// received from other peers and the protocol has not completed the keygen stage
	// We take it that the protocol has stalled if messages are not received from other peers after an interval of 3 blocks
	// And the keygen stage has not completed
	pub fn has_stalled(&self, time_to_restart: Option<C>, current_block_number: C) -> bool {
		let last_stage = self.stage_at_last_receipt;
		let current_stage = self.stage;
		let block_diff = current_block_number - self.last_received_at;

		if block_diff >= time_to_restart.unwrap_or(3u32.into()) &&
			last_stage == current_stage &&
			self.is_key_gen_stage()
		{
			return true
		}

		false
	}

	pub fn proceed(&mut self, at: C) -> Result<(), DKGError> {
		let proceed_res = match self.stage {
			Stage::Keygen => self.proceed_keygen(at),
			Stage::Offline => self.proceed_offline_stage(at),
			Stage::ManualReady => self.proceed_vote(at),
			_ => Ok(false),
		};

		return match proceed_res {
			Ok(finished) => {
				if finished {
					self.advance_stage()
				}
				Ok(())
			},
			Err(err) => Err(err),
		}
	}

	pub fn get_outgoing_messages(&mut self) -> Vec<DKGMsgPayload<K>> {
		trace!(target: "dkg", "🕸️  Get outgoing, stage {:?}", self.stage);

		match self.stage {
			Stage::Keygen => self
				.get_outgoing_messages_keygen()
				.into_iter()
				.map(|msg| DKGMsgPayload::Keygen(msg))
				.collect(),
			Stage::Offline => self
				.get_outgoing_messages_offline_stage()
				.into_iter()
				.map(|msg| DKGMsgPayload::Offline(msg))
				.collect(),
			Stage::ManualReady => self
				.get_outgoing_messages_vote()
				.into_iter()
				.map(|msg| DKGMsgPayload::Vote(msg))
				.collect(),
			_ => vec![],
		}
	}

	pub fn handle_incoming(
		&mut self,
		data: DKGMsgPayload<K>,
		current_block_number: Option<C>,
	) -> Result<(), DKGError> {
		trace!(target: "dkg", "🕸️  Handle incoming, stage {:?}", self.stage);
		if current_block_number.is_some() {
			self.last_received_at = current_block_number.unwrap();
		}

		self.stage_at_last_receipt = self.stage;
		return match data {
			DKGMsgPayload::Keygen(msg) => {
				// TODO: check keygen_set_id
				if Stage::Keygen == self.stage {
					self.handle_incoming_keygen(msg)
				} else {
					self.pending_keygen_msgs.push(msg);
					Ok(())
				}
			},
			DKGMsgPayload::Offline(msg) => {
				// TODO: check signer_set_id
				if Stage::Offline == self.stage {
					self.handle_incoming_offline_stage(msg)
				} else {
					self.pending_offline_msgs.push(msg);
					Ok(())
				}
			},
			DKGMsgPayload::Vote(msg) =>
				if Stage::ManualReady == self.stage {
					self.handle_incoming_vote(msg)
				} else {
					Ok(())
				},
			_ => Ok(()),
		}
	}

	pub fn start_keygen(
		&mut self,
		keygen_set_id: KeygenSetId,
		started_at: C,
	) -> Result<(), DKGError> {
		info!(
			target: "dkg",
			"🕸️  Starting new DKG w/ party_index {:?}, threshold {:?}, size {:?}",
			self.party_index,
			self.threshold,
			self.parties,
		);
		trace!(target: "dkg", "🕸️  Keygen set id: {}", keygen_set_id);

		match Keygen::new(self.party_index, self.threshold, self.parties) {
			Ok(new_keygen) => {
				self.stage = Stage::Keygen;
				self.keygen_set_id = keygen_set_id;
				self.keygen_started_at = started_at;
				self.keygen = Some(new_keygen);

				// Processing pending messages
				for msg in std::mem::take(&mut self.pending_keygen_msgs) {
					if let Err(err) = self.handle_incoming_keygen(msg) {
						warn!(target: "dkg", "🕸️  Error handling pending keygen msg {:?}", err);
					}
					self.proceed_keygen(started_at)?;
				}
				trace!(target: "dkg", "🕸️  Handled {} pending keygen messages", self.pending_keygen_msgs.len());
				self.pending_keygen_msgs.clear();

				Ok(())
			},
			Err(err) => Err(DKGError::StartKeygen { reason: err.to_string() }),
		}
	}

	pub fn reset_signers(
		&mut self,
		signer_set_id: SignerSetId,
		s_l: Vec<u16>,
		started_at: C,
	) -> Result<(), DKGError> {
		info!(target: "dkg", "🕸️  Resetting singers {:?}", s_l);
		info!(target: "dkg", "🕸️  Signer set id {:?}", signer_set_id);

		match self.stage {
			Stage::KeygenReady | Stage::Keygen => Err(DKGError::ResetSigners {
				reason: "Cannot reset signers and start offline stage, Keygen is not complete"
					.to_string(),
			}),
			_ =>
				if let Some(local_key_clone) = self.local_key.clone() {
					return match OfflineStage::new(self.party_index, s_l.clone(), local_key_clone) {
						Ok(new_offline_stage) => {
							self.stage = Stage::Offline;
							self.signer_set_id = signer_set_id;
							self.signers = s_l;
							self.offline_started_at = started_at;
							self.offline_stage = Some(new_offline_stage);
							self.completed_offline_stage = None;

							for msg in std::mem::take(&mut self.pending_offline_msgs) {
								if let Err(err) = self.handle_incoming_offline_stage(msg) {
									warn!(target: "dkg", "🕸️  Error handling pending offline msg {:?}", err);
								}
								self.proceed_offline_stage(started_at)?;
							}
							self.pending_offline_msgs.clear();
							trace!(target: "dkg", "🕸️  Handled {} pending offline messages", self.pending_offline_msgs.len());

							Ok(())
						},
						Err(err) => {
							error!("Error creating new offline stage {}", err);
							Err(DKGError::ResetSigners { reason: err.to_string() })
						},
					}
				} else {
					Err(DKGError::ResetSigners { reason: "No local key present".to_string() })
				},
		}
	}

	pub fn vote(&mut self, round_key: K, data: Vec<u8>, started_at: C) -> Result<(), String> {
		if let Some(completed_offline) = self.completed_offline_stage.as_mut() {
			let round = self.rounds.entry(round_key).or_default();
			let hash = BigInt::from_bytes(&keccak_256(&data));

			match SignManual::new(hash, completed_offline.clone()) {
				Ok((sign_manual, sig)) => {
					trace!(target: "dkg", "🕸️  Creating vote /w key {:?}", &round_key);

					round.sign_manual = Some(sign_manual);
					round.payload = Some(data);
					round.started_at = started_at;

					match bincode::serialize(&sig) {
						Ok(serialized_sig) => {
							let msg = DKGVoteMessage {
								party_ind: self.party_index,
								round_key,
								partial_signature: serialized_sig,
							};
							self.sign_outgoing_msgs.push(msg);
							return Ok(())
						},
						Err(err) => return Err(err.to_string()),
					}
				},
				Err(err) => return Err(err.to_string()),
			}
		}
		Err("Not ready to vote".to_string())
	}

	pub fn is_key_gen_stage(&self) -> bool {
		Stage::Keygen == self.stage
	}

	pub fn is_offline_ready(&self) -> bool {
		Stage::OfflineReady == self.stage
	}

	pub fn is_ready_to_vote(&self) -> bool {
		Stage::ManualReady == self.stage
	}

	pub fn has_finished_rounds(&self) -> bool {
		!self.finished_rounds.is_empty()
	}

	pub fn get_finished_rounds(&mut self) -> Vec<DKGSignedPayload<K>> {
		std::mem::take(&mut self.finished_rounds)
	}

	pub fn dkg_params(&self) -> (u16, u16, u16) {
		(self.party_index, self.threshold, self.parties)
	}

	pub fn get_public_key(&self) -> Option<Secp256k1Point> {
		if let Some(local_key) = &self.local_key {
			Some(local_key.public_key().clone())
		} else {
			None
		}
	}

	pub fn get_id(&self) -> RoundId {
		self.round_id
	}

	pub fn has_vote_in_process(&self, round_key: K) -> bool {
		return self.rounds.contains_key(&round_key)
	}
}

impl<K, C> MultiPartyECDSARounds<K, C>
where
	K: Ord + Encode + Copy + core::fmt::Debug,
	C: AtLeast32BitUnsigned + Copy,
{
	/// Internal ///

	fn advance_stage(&mut self) {
		self.stage = self.stage.get_next();
		info!(target: "dkg", "🕸️  New stage {:?}", self.stage);
	}

	/// Proceed to next step for current Stage

	fn proceed_keygen(&mut self, at: C) -> Result<bool, DKGError> {
		trace!(target: "dkg", "🕸️  Keygen party {} enter proceed", self.party_index);

		let keygen = self.keygen.as_mut().unwrap();

		if keygen.wants_to_proceed() {
			info!(target: "dkg", "🕸️  Keygen party {} wants to proceed", keygen.party_ind());
			trace!(target: "dkg", "🕸️  before: {:?}", keygen);
			// TODO, handle asynchronously
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

		if self.try_finish_keygen() {
			Ok(true)
		} else {
			if at - self.keygen_started_at > KEYGEN_TIMEOUT.into() {
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

	fn proceed_offline_stage(&mut self, at: C) -> Result<bool, DKGError> {
		trace!(target: "dkg", "🕸️  OfflineStage party {} enter proceed", self.party_index);

		if self.offline_stage.is_none() {
			return Err(DKGError::GenericError { reason: "Offline Stage missing".to_string() })
		}

		let offline_stage = self.offline_stage.as_mut().unwrap();

		if offline_stage.wants_to_proceed() {
			info!(target: "dkg", "🕸️  OfflineStage party {} wants to proceed", offline_stage.party_ind());
			trace!(target: "dkg", "🕸️  before: {:?}", offline_stage);
			// TODO, handle asynchronously
			match offline_stage.proceed() {
				Ok(_) => {
					trace!(target: "dkg", "🕸️  after: {:?}", offline_stage);
				},
				Err(err) => {
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

		if self.try_finish_offline_stage() {
			Ok(true)
		} else {
			if at - self.offline_started_at > OFFLINE_TIMEOUT.into() {
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

	fn proceed_vote(&mut self, at: C) -> Result<bool, DKGError> {
		if let Err(err) = self.try_finish_vote() {
			return Err(err)
		} else {
			let mut timed_out: Vec<K> = Vec::new();

			for (round_key, round) in self.rounds.iter() {
				if round.is_signed_by(self.party_index) &&
					at - round.started_at > SIGN_TIMEOUT.into()
				{
					timed_out.push(round_key.clone());
				}
			}

			if !timed_out.is_empty() {
				let mut bad_actors: Vec<u16> = Vec::new();

				for round_key in timed_out.iter() {
					if let Some(round) = self.rounds.remove(round_key) {
						let signed_by = round.get_signed_parties();

						let mut not_signed_by: Vec<u16> = self
							.signers
							.iter()
							.filter(|v| !signed_by.contains(*v))
							.map(|v| *v)
							.collect();

						bad_actors.append(&mut not_signed_by)
					}
				}

				Err(DKGError::SignTimeout { bad_actors })
			} else {
				Ok(false)
			}
		}
	}

	/// Try finish current Stage

	fn try_finish_keygen(&mut self) -> bool {
		let keygen = self.keygen.as_mut().unwrap();

		if keygen.is_finished() {
			info!(target: "dkg", "🕸️  Keygen is finished, extracting output, round_id: {:?}", self.round_id);
			match keygen.pick_output() {
				Some(Ok(k)) => {
					self.local_key = Some(k.clone());

					// We only persist the local key if we have all that is required to encrypt it
					if self.local_key_path.is_some() &&
						self.local_keystore.is_some() &&
						self.public_key.is_some()
					{
						let key_pair = self.local_keystore.as_ref().unwrap().key_pair::<AppPair>(
							&Public::try_from(&self.public_key.as_ref().unwrap().0[..]).unwrap(),
						);
						if let Ok(Some(key_pair)) = key_pair {
							let _ = store_localkey(
								k,
								self.round_id,
								self.local_key_path.as_ref().unwrap().clone(),
								key_pair.to_raw_vec(),
							);
						}
					}
					info!(target: "dkg", "🕸️  local share key is extracted");
					return true
				},
				Some(Err(e)) => panic!("Keygen finished with error result {}", e),
				None => panic!("Keygen finished with no result"),
			}
		}
		return false
	}

	fn try_finish_offline_stage(&mut self) -> bool {
		if self.offline_stage.is_none() {
			return false
		}

		let offline_stage = self.offline_stage.as_mut().unwrap();

		if offline_stage.is_finished() {
			info!(target: "dkg", "🕸️  OfflineStage is finished, extracting output round_id: {:?}", self.round_id);
			match offline_stage.pick_output() {
				Some(Ok(cos)) => {
					self.completed_offline_stage = Some(cos.clone());
					info!(target: "dkg", "🕸️  CompletedOfflineStage is extracted");
					return true
				},
				Some(Err(e)) => panic!("OfflineStage finished with error result {}", e),
				None => panic!("OfflineStage finished with no result"),
			}
		}
		return false
	}

	fn try_finish_vote(&mut self) -> Result<bool, DKGError> {
		let mut finished: Vec<K> = Vec::new();

		for (round_key, round) in self.rounds.iter() {
			if round.is_done(self.threshold.into()) {
				finished.push(round_key.clone());
			}
		}

		trace!(target: "dkg", "🕸️  {} Rounds done", finished.len());

		for round_key in finished.iter() {
			if let Some(mut round) = self.rounds.remove(round_key) {
				let payload = round.payload.take();
				let sig = round.complete();

				if let Err(err) = sig {
					return Err(err)
				} else if let (Some(payload), Ok(sig)) = (payload, sig) {
					match convert_signature(&sig) {
						Some(signature) => {
							let signed_payload = DKGSignedPayload {
								key: round_key.clone(),
								payload,
								signature: signature.encode(),
							};

							self.finished_rounds.push(signed_payload);

							trace!(target: "dkg", "🕸️  Finished round /w key: {:?}", &round_key);
						},
						_ => debug!("Error serializing signature"),
					}
				}
			}
		}

		Ok(false)
	}

	/// Get outgoing messages for current Stage

	fn get_outgoing_messages_keygen(&mut self) -> Vec<DKGKeygenMessage> {
		if let Some(keygen) = self.keygen.as_mut() {
			trace!(target: "dkg", "🕸️  Getting outgoing keygen messages");

			if !keygen.message_queue().is_empty() {
				trace!(target: "dkg", "🕸️  Outgoing messages, queue len: {}", keygen.message_queue().len());

				let keygen_set_id = self.keygen_set_id;

				let enc_messages = keygen
					.message_queue()
					.into_iter()
					.map(|m| {
						trace!(target: "dkg", "🕸️  MPC protocol message {:?}", m);
						let m_ser = bincode::serialize(m).unwrap();
						return DKGKeygenMessage { keygen_set_id, keygen_msg: m_ser }
					})
					.collect::<Vec<DKGKeygenMessage>>();

				keygen.message_queue().clear();
				return enc_messages
			}
		}
		vec![]
	}

	fn get_outgoing_messages_offline_stage(&mut self) -> Vec<DKGOfflineMessage> {
		if let Some(offline_stage) = self.offline_stage.as_mut() {
			trace!(target: "dkg", "🕸️  Getting outgoing offline messages");

			if !offline_stage.message_queue().is_empty() {
				trace!(target: "dkg", "🕸️  Outgoing messages, queue len: {}", offline_stage.message_queue().len());

				let singer_set_id = self.signer_set_id;

				let enc_messages = offline_stage
					.message_queue()
					.into_iter()
					.map(|m| {
						trace!(target: "dkg", "🕸️  MPC protocol message {:?}", *m);
						let m_ser = bincode::serialize(m).unwrap();
						return DKGOfflineMessage {
							signer_set_id: singer_set_id,
							offline_msg: m_ser,
						}
					})
					.collect::<Vec<DKGOfflineMessage>>();

				offline_stage.message_queue().clear();
				return enc_messages
			}
		}
		vec![]
	}

	fn get_outgoing_messages_vote(&mut self) -> Vec<DKGVoteMessage<K>> {
		trace!(target: "dkg", "🕸️  Getting outgoing vote messages");
		std::mem::take(&mut self.sign_outgoing_msgs)
	}

	/// Handle incoming messages for current Stage

	fn handle_incoming_keygen(&mut self, data: DKGKeygenMessage) -> Result<(), DKGError> {
		if data.keygen_set_id != self.keygen_set_id {
			return Err(DKGError::GenericError { reason: "Keygen set ids do not match".to_string() })
		}

		if let Some(keygen) = self.keygen.as_mut() {
			trace!(target: "dkg", "🕸️  Handle incoming keygen message");
			if data.keygen_msg.is_empty() {
				warn!(
					target: "dkg", "🕸️  Got empty message");
				return Ok(())
			}
			let msg: Msg<ProtocolMessage> = match bincode::deserialize(&data.keygen_msg) {
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
		}
		Ok(())
	}

	fn handle_incoming_offline_stage(&mut self, data: DKGOfflineMessage) -> Result<(), DKGError> {
		if data.signer_set_id != self.signer_set_id {
			return Err(DKGError::GenericError { reason: "Signer set ids do not match".to_string() })
		}

		if let Some(offline_stage) = self.offline_stage.as_mut() {
			trace!(target: "dkg", "🕸️  Handle incoming offline message");
			if data.offline_msg.is_empty() {
				warn!(target: "dkg", "🕸️  Got empty message");
				return Ok(())
			}
			let msg: Msg<OfflineProtocolMessage> = match bincode::deserialize(&data.offline_msg) {
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
					return Err(DKGError::GenericError {
						reason: "Offline critical error encountered".to_string(),
					})
				},
				Err(err) => {
					error!(target: "dkg", "🕸️  Non-critical error encountered: {:?}", err);
				},
			}
			debug!(target: "dkg", "🕸️  State after incoming message processing: {:?}", offline_stage);
		}
		Ok(())
	}

	fn handle_incoming_vote(&mut self, data: DKGVoteMessage<K>) -> Result<(), DKGError> {
		trace!(target: "dkg", "🕸️  Handle vote message");

		if data.party_ind == self.party_index {
			warn!(target: "dkg", "🕸️  Ignore messages sent by self");
			return Ok(())
		}

		let sig: PartialSignature = match bincode::deserialize(&data.partial_signature) {
			Ok(sig) => sig,
			Err(err) => {
				error!(target: "dkg", "🕸️  Error deserializing msg: {:?}", err);
				return Err(DKGError::GenericError {
					reason: "Error deserializing vote msg".to_string(),
				})
			},
		};

		self.rounds.entry(data.round_key).or_default().add_vote(data.party_ind, sig);

		Ok(())
	}
}

struct DKGRoundTracker<Payload, Clock> {
	votes: BTreeMap<u16, PartialSignature>,
	sign_manual: Option<SignManual>,
	payload: Option<Payload>,
	started_at: Clock,
}

impl<P, C> Default for DKGRoundTracker<P, C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn default() -> Self {
		Self {
			votes: Default::default(),
			sign_manual: Default::default(),
			payload: Default::default(),
			started_at: 0u32.into(),
		}
	}
}

impl<P, C> DKGRoundTracker<P, C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	fn add_vote(&mut self, party: u16, vote: PartialSignature) -> bool {
		self.votes.insert(party, vote);
		true
	}

	fn is_signed_by(&self, party: u16) -> bool {
		self.votes.contains_key(&party)
	}

	fn get_signed_parties(&self) -> Vec<u16> {
		self.votes.keys().map(|v| *v).collect()
	}

	fn is_done(&self, threshold: usize) -> bool {
		self.sign_manual.is_some() && self.votes.len() >= threshold
	}

	fn complete(mut self) -> Result<SignatureRecid, DKGError> {
		if let Some(sign_manual) = self.sign_manual.take() {
			debug!(target: "dkg", "Tyring to complete vote with {} votes", self.votes.len());

			let votes: Vec<PartialSignature> = self.votes.into_values().collect();

			return match sign_manual.complete(&votes) {
				Ok(sig) => {
					debug!("Obtained complete signature: {}", &sig.recid);
					Ok(sig)
				},
				Err(err) => {
					let sign_err = match err {
						SignError::LocalSigning(sign_err) => sign_err,
						SignError::CompleteSigning(sign_err) => sign_err,
					};

					match sign_err {
						gg20_sign::rounds::Error::Round1(err_type) =>
							return Err(DKGError::SignMisbehaviour {
								bad_actors: vec_usize_to_u16(err_type.bad_actors),
							}),
						gg20_sign::rounds::Error::Round2Stage4(err_type) =>
							return Err(DKGError::SignMisbehaviour {
								bad_actors: vec_usize_to_u16(err_type.bad_actors),
							}),
						gg20_sign::rounds::Error::Round3(err_type) =>
							return Err(DKGError::SignMisbehaviour {
								bad_actors: vec_usize_to_u16(err_type.bad_actors),
							}),
						gg20_sign::rounds::Error::Round5(err_type) =>
							return Err(DKGError::SignMisbehaviour {
								bad_actors: vec_usize_to_u16(err_type.bad_actors),
							}),
						gg20_sign::rounds::Error::Round6VerifyProof(err_type) =>
							return Err(DKGError::SignMisbehaviour {
								bad_actors: vec_usize_to_u16(err_type.bad_actors),
							}),
						_ => return Err(DKGError::GenericError { reason: sign_err.to_string() }),
					};
				},
			}
		}
		Err(DKGError::GenericError { reason: "No SignManual found".to_string() })
	}
}

pub fn convert_signature(sig_recid: &SignatureRecid) -> Option<Signature> {
	let r = sig_recid.r.to_big_int().to_bytes();
	let s = sig_recid.s.to_big_int().to_bytes();
	let v = sig_recid.recid;

	let mut sig_vec: Vec<u8> = Vec::new();

	for _ in 0..(32 - r.len()) {
		sig_vec.extend(&[0]);
	}
	sig_vec.extend_from_slice(&r);

	for _ in 0..(32 - s.len()) {
		sig_vec.extend(&[0]);
	}
	sig_vec.extend_from_slice(&s);

	sig_vec.extend(&[v]);

	if 65 != sig_vec.len() {
		warn!(target: "dkg", "🕸️  Invalid signature len: {}, expected 65", sig_vec.len());
		return None
	}

	let mut dkg_sig_arr: [u8; 65] = [0; 65];
	dkg_sig_arr.copy_from_slice(&sig_vec[0..65]);

	return match Signature(dkg_sig_arr).try_into() {
		Ok(sig) => {
			debug!(target: "dkg", "🕸️  Converted signature {:?}", &sig);
			Some(sig)
		},
		Err(err) => {
			warn!(target: "dkg", "🕸️  Error converting signature {:?}", err);
			None
		},
	}
}

#[cfg(test)]
mod tests {
	use super::{MultiPartyECDSARounds, Stage};
	use crate::types::{DKGError, DKGMsgPayload};
	use codec::Encode;

	fn check_all_reached_stage(
		parties: &Vec<MultiPartyECDSARounds<u64, u32>>,
		target_stage: Stage,
	) -> bool {
		for party in parties.iter() {
			if party.stage != target_stage {
				return false
			}
		}
		true
	}

	fn check_all_parties_have_public_key(parties: &Vec<MultiPartyECDSARounds<u64, u32>>) {
		for party in parties.iter() {
			if party.get_public_key().is_none() {
				panic!("No public key for party {}", party.party_index)
			}
		}
	}

	fn check_all_reached_offline_ready(parties: &Vec<MultiPartyECDSARounds<u64, u32>>) -> bool {
		check_all_reached_stage(parties, Stage::OfflineReady)
	}

	fn check_all_reached_manual_ready(parties: &Vec<MultiPartyECDSARounds<u64, u32>>) -> bool {
		check_all_reached_stage(parties, Stage::ManualReady)
	}

	fn check_all_signatures_ready(parties: &Vec<MultiPartyECDSARounds<u64, u32>>) -> bool {
		for party in parties.iter() {
			if !party.has_finished_rounds() {
				return false
			}
		}
		true
	}

	fn check_all_signatures_correct(parties: &mut Vec<MultiPartyECDSARounds<u64, u32>>) {
		for party in &mut parties.into_iter() {
			let mut finished_rounds = party.get_finished_rounds();

			if finished_rounds.len() == 1 {
				let finished_round = finished_rounds.remove(0);

				let message = b"Webb".encode();

				assert!(
					dkg_runtime_primitives::utils::validate_ecdsa_signature(
						&message,
						&finished_round.signature
					),
					"Invalid signature for party {}",
					party.party_index
				);

				println!("Party {}; sig: {:?}", party.party_index, &finished_round.signature);
			} else {
				panic!("No signature extracted")
			}
		}

		for party in &mut parties.into_iter() {
			match party.stage {
				Stage::ManualReady => (),
				_ => panic!("Stage must be ManualReady, but {:?} found", &party.stage),
			}
		}

		println!("All signatures are correct");
	}

	fn run_simulation<C>(parties: &mut Vec<MultiPartyECDSARounds<u64, u32>>, stop_condition: C)
	where
		C: Fn(&Vec<MultiPartyECDSARounds<u64, u32>>) -> bool,
	{
		println!("Simulation starts");

		let mut msgs_pull = vec![];
		let mut all_messages = vec![];

		for party in &mut parties.into_iter() {
			party.proceed(0).unwrap();

			msgs_pull.append(&mut party.get_outgoing_messages());
		}

		for _i in 1..100 {
			let msgs_pull_frozen = msgs_pull.split_off(0);

			for party in &mut parties.into_iter() {
				for msg_frozen in msgs_pull_frozen.iter() {
					all_messages.push(msg_frozen.clone());
					match party.handle_incoming(msg_frozen.clone(), None) {
						Ok(()) => (),
						Err(err) => panic!("{:?}", err),
					}
				}
				msgs_pull.append(&mut party.get_outgoing_messages());
			}

			for party in &mut parties.into_iter() {
				party.proceed(0).unwrap();

				msgs_pull.append(&mut party.get_outgoing_messages());
			}

			if stop_condition(parties) {
				println!("All parties finished");
				return Ok(())
			}
		}

		Err(("Test failed", all_messages))
	}

	fn simulate_multi_party(t: u16, n: u16, s_l: Vec<u16>) {
		let mut parties: Vec<MultiPartyECDSARounds<u64, u32>> = vec![];

		for i in 1..=n {
			let mut party = MultiPartyECDSARounds::new(i, t, n, i as u64, None, 0, None, None);
			println!("Starting keygen for party {}, Stage: {:?}", party.party_index, party.stage);
			party.start_keygen(0, 0).unwrap();
			parties.push(party);
		}

		// Running Keygen stage
		println!("Running Keygen");
		run_simulation(&mut parties, check_all_reached_offline_ready);
		check_all_parties_have_public_key(&mut &parties);

		// Running Offline stage
		println!("Running Offline");
		let parties_refs = &mut parties;
		for party in parties_refs.into_iter() {
			println!("Resetting signers for party {}, Stage: {:?}", party.party_index, party.stage);
			match party.reset_signers(0, s_l.clone(), 0) {
				Ok(()) => (),
				Err(_err) => (),
			}
		}
		run_simulation(&mut parties, check_all_reached_manual_ready);

		// Running Sign stage
		println!("Running Sign");
		let parties_refs = &mut parties;
		for party in &mut parties_refs.into_iter() {
			println!("Vote for party {}, Stage: {:?}", party.party_index, party.stage);
			party.vote(1, "Webb".encode(), 0).unwrap();
		}
		run_simulation(&mut parties, check_all_signatures_ready);

		// Extract all signatures and check for correctness
		check_all_signatures_correct(&mut parties);
	}

	#[test]
	fn simulate_multi_party_t2_n3() {
		simulate_multi_party(2, 3, (1..=3).collect());
	}

	#[test]
	fn simulate_multi_party_t3_n5() {
		simulate_multi_party(3, 5, (1..=5).collect());
	}
}
