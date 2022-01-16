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

/// DKG State tracker
pub struct DKGState<C> {
	pub accepted: bool,
	pub is_epoch_over: bool,
	pub listening_for_pub_key: bool,
	pub listening_for_active_pub_key: bool,
	pub curr_dkg: Option<MultiPartyState<C>>,
	pub past_dkg: Option<MultiPartyState<C>>,
	pub created_offlinestage_at: HashMap<Vec<u8>, C>,
}

const KEYGEN_TIMEOUT: u32 = 10;
const OFFLINE_TIMEOUT: u32 = 10;
const SIGN_TIMEOUT: u32 = 3;

pub enum MultiPartyState<C> {
	Keygen(MulitPartyKeygen<C>),
	Sign(MultiPartySign<C>),
}

/// State machine structure for performing Keygen
/// HashMap and BtreeMap keys are encoded formats of (ChainId, DKGPayloadKey)
/// Using DKGPayloadKey only will cause collisions when proposals with the same nonce but from
/// different chains are submitted

pub struct MultiPartyKeygen<Clock> {
	round_id: RoundId,
	party_index: u16,
	threshold: u16,
	parties: u16,

	keygen_set_id: KeygenSetId,
	keygen: KeygenState<Clock>,

	// DKG clock
	keygen_started_at: Clock,
	// The block number at which a dkg message was last received
	last_received_at: Clock,

	// Message processing
	pending_keygen_msgs: Vec<DKGKeygenMessage>,

	// File system storage and encryption
	local_key_path: Option<PathBuf>,
	public_key: Option<sr25519::Public>,
	local_keystore: Option<Arc<LocalKeystore>>,
}

impl<C> MultiPartyKeygen<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	/// A check to know if the protocol has stalled at the keygen stage,
	/// We take it that the protocol has stalled if keygen messages are not received from other peers after a certain interval
	/// And the keygen stage has not completed
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

	pub fn proceed(&mut self, at: C) -> Vec<Result<(), DKGError>> {
		let proceed_res = match self.stage {
			Stage::Keygen => self.proceed_keygen(at),
			Stage::OfflineReady => Ok(false),
			_ => Ok(false),
		};

		let mut results = vec![];

		match proceed_res {
			Ok(finished) =>
				if finished {
					self.advance_stage();
				},
			Err(err) => results.push(Err(err)),
		}

		let keys = self.offline_stage.keys().cloned().collect::<Vec<_>>();
		for key in &keys {
			let res = self.proceed_offline_stage(key.clone(), at).map(|_| ());
			if res.is_err() {
				results.push(res);
			}
		}

		let res = self.proceed_vote(at).map(|_| ());

		if res.is_err() {
			results.push(res);
		}

		results
	}

	pub fn get_outgoing_messages(&mut self) -> Vec<DKGMsgPayload> {
		trace!(target: "dkg", "🕸️  Get outgoing Keygen");

		let mut all_messages = match self.keygen {
			KeygenState::Started(keygen) => keygen
				.get_outgoing_messages_keygen()
				.into_iter()
				.map(|msg| DKGMsgPayload::Keygen(msg))
				.collect(),
			_ => vec![],
		};

		all_messages
	}

	pub fn handle_incoming(
		&mut self,
		data: DKGMsgPayload,
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

	pub fn dkg_params(&self) -> (u16, u16, u16) {
		(self.party_index, self.threshold, self.parties)
	}

	pub fn get_public_key(&self) -> Option<GE> {
		if let Some(local_key) = &self.local_key {
			Some(local_key.public_key().clone())
		} else {
			None
		}
	}

	pub fn get_id(&self) -> RoundId {
		self.round_id
	}
}

/// State machine structure for performing Offline and Sign
/// HashMap and BtreeMap keys are encoded formats of (ChainId, DKGPayloadKey)
/// Using DKGPayloadKey only will cause collisions when proposals with the same nonce but from
/// different chains are submitted
pub struct MultiPartySign<Clock> {
	round_id: RoundId,
	party_index: u16,
	threshold: u16,
	parties: u16,

	signer_set_id: SignerSetId,
	signers: Vec<u16>,
	offline: HashMap<Vec<u8>, OfflineState<Clock>>,
	sign: HashMap<Vec<u8>, SignState<Clock>>,

	// DKG clock
	offline_started_at: HashMap<Vec<u8>, Clock>,
	// The block number at which a dkg message was last received
	last_received_at: Clock,

	// Message processing
	pending_offline_msgs: HashMap<Vec<u8>, Vec<DKGOfflineMessage>>,

	local_key: LocalKey<Secp256k1>,
}

impl<C> MultiPartySign<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn set_local_key(&mut self, local_key: LocalKey<Secp256k1>) {
		self.local_key = Some(local_key)
	}

	pub fn set_stage(&mut self, stage: Stage) {
		self.stage = stage;
	}

	pub fn set_signers(&mut self, signers: Vec<u16>) {
		self.signers = signers;
	}

	pub fn set_signer_set_id(&mut self, set_id: SignerSetId) {
		self.signer_set_id = set_id;
	}

	/// A check to know if the protocol has stalled at the keygen stage,
	/// We take it that the protocol has stalled if keygen messages are not received from other peers after a certain interval
	/// And the keygen stage has not completed
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

	pub fn proceed(&mut self, at: C) -> Vec<Result<(), DKGError>> {
		let proceed_res = match self.stage {
			Stage::Keygen => self.proceed_keygen(at),
			Stage::OfflineReady => Ok(false),
			_ => Ok(false),
		};

		let mut results = vec![];

		match proceed_res {
			Ok(finished) =>
				if finished {
					self.advance_stage();
				},
			Err(err) => results.push(Err(err)),
		}

		let keys = self.offline_stage.keys().cloned().collect::<Vec<_>>();
		for key in &keys {
			let res = self.proceed_offline_stage(key.clone(), at).map(|_| ());
			if res.is_err() {
				results.push(res);
			}
		}

		let res = self.proceed_vote(at).map(|_| ());

		if res.is_err() {
			results.push(res);
		}

		results
	}

	pub fn get_outgoing_messages(&mut self) -> Vec<DKGMsgPayload> {
		trace!(target: "dkg", "🕸️  Get outgoing, stage {:?}", self.stage);

		let mut all_messages = match self.stage {
			Stage::Keygen => self
				.get_outgoing_messages_keygen()
				.into_iter()
				.map(|msg| DKGMsgPayload::Keygen(msg))
				.collect(),
			_ => vec![],
		};

		let offline_messages = self
			.get_outgoing_messages_offline_stage()
			.into_iter()
			.map(|msg| DKGMsgPayload::Offline(msg))
			.collect::<Vec<_>>();

		let vote_messages = self
			.get_outgoing_messages_vote()
			.into_iter()
			.map(|msg| DKGMsgPayload::Vote(msg))
			.collect::<Vec<_>>();

		all_messages.extend_from_slice(&offline_messages[..]);
		all_messages.extend_from_slice(&vote_messages[..]);

		all_messages
	}

	pub fn handle_incoming(
		&mut self,
		data: DKGMsgPayload,
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
			DKGMsgPayload::Offline(msg) =>
				if self.offline_stage.contains_key(&msg.key) {
					let res = self.handle_incoming_offline_stage(msg.clone());
					if let Err(DKGError::CriticalError { reason: _ }) = res.clone() {
						self.offline_stage.remove(&msg.key);
						self.local_stages.remove(&msg.key);
					}
					res
				} else {
					let messages = self.pending_offline_msgs.entry(msg.key.clone()).or_default();
					messages.push(msg);
					Ok(())
				},
			DKGMsgPayload::Vote(msg) => self.handle_incoming_vote(msg),
			_ => Ok(()),
		}
	}

	pub fn create_offline_stage(&mut self, key: Vec<u8>, started_at: C) -> Result<(), DKGError> {
		info!(target: "dkg", "🕸️  Creating offline stage for {:?}", &key);
		match self.stage {
			Stage::KeygenReady | Stage::Keygen => Err(DKGError::CreateOfflineStage {
				reason: "Cannot start offline stage, Keygen is not complete".to_string(),
			}),
			_ =>
				if let Some(local_key_clone) = self.local_key.clone() {
					let s_l = (1..=self.dkg_params().2).collect::<Vec<_>>();
					return match OfflineStage::new(self.party_index, s_l, local_key_clone) {
						Ok(new_offline_stage) => {
							self.local_stages.insert(key.clone(), MiniStage::Offline);
							self.offline_started_at.insert(key.clone(), started_at);
							self.offline_stage.insert(key.clone(), new_offline_stage);

							for msg in self.pending_offline_msgs.remove(&key).unwrap_or_default() {
								if let Err(err) = self.handle_incoming_offline_stage(msg) {
									warn!(target: "dkg", "🕸️  Error handling pending offline msg {:?}", err);
								}
								self.proceed_offline_stage(key.clone(), started_at)?;
							}
							trace!(target: "dkg", "🕸️  Handled pending offline messages for {:?}", key);

							Ok(())
						},
						Err(err) => {
							error!("Error creating new offline stage {}", err);
							Err(DKGError::CreateOfflineStage { reason: err.to_string() })
						},
					}
				} else {
					Err(DKGError::CreateOfflineStage { reason: "No local key present".to_string() })
				},
		}
	}

	pub fn vote(&mut self, round_key: Vec<u8>, data: Vec<u8>, started_at: C) -> Result<(), String> {
		let proceed_res =
			if let Some(completed_offline) = self.completed_offline_stage.remove(&round_key) {
				let round = self.rounds.entry(round_key.clone()).or_default();
				let hash = BigInt::from_bytes(&keccak_256(&data));

				match SignManual::new(hash, completed_offline.clone()) {
					Ok((sign_manual, sig)) => {
						trace!(target: "dkg", "🕸️  Creating vote /w key {:?}", &round_key);

						round.sign_manual = Some(sign_manual);
						round.payload = Some(data);
						round.started_at = started_at;

						let serialized = serde_json::to_string(&sig).unwrap();
						let msg = DKGVoteMessage {
							party_ind: self.party_index,
							round_key: round_key.clone(),
							partial_signature: serialized.into_bytes(),
						};
						self.sign_outgoing_msgs.push(msg);
						Ok(true)
					},
					Err(err) => Err(err.to_string()),
				}
			} else {
				Err("Not ready to vote".to_string())
			};

		match proceed_res {
			Ok(true | false) => {
				self.local_stages.remove(&round_key);
				Ok(())
			},
			Err(err) => Err(err),
		}
	}

	pub fn has_finished_rounds(&self) -> bool {
		!self.finished_rounds.is_empty()
	}

	pub fn get_finished_rounds(&mut self) -> Vec<DKGSignedPayload> {
		std::mem::take(&mut self.finished_rounds)
	}

	pub fn dkg_params(&self) -> (u16, u16, u16) {
		(self.party_index, self.threshold, self.parties)
	}

	pub fn is_signer(&self) -> bool {
		self.signers.contains(&self.party_index)
	}

	pub fn get_public_key(&self) -> Option<GE> {
		if let Some(local_key) = &self.local_key {
			Some(local_key.public_key().clone())
		} else {
			None
		}
	}

	pub fn get_id(&self) -> RoundId {
		self.round_id
	}

	pub fn has_vote_in_process(&self, round_key: Vec<u8>) -> bool {
		return self.rounds.contains_key(&round_key)
	}
}

pub fn convert_signature(sig_recid: &SignatureRecid) -> Option<Signature> {
	let r = sig_recid.r.to_bigint().to_bytes();
	let s = sig_recid.s.to_bigint().to_bytes();
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
	use codec::Encode;

	fn check_all_reached_stage(
		parties: &Vec<MultiPartyECDSARounds<u32>>,
		target_stage: Stage,
	) -> bool {
		for party in parties.iter() {
			if party.stage != target_stage {
				return false
			}
		}
		true
	}

	fn check_all_parties_have_public_key(parties: &Vec<MultiPartyECDSARounds<u32>>) {
		for party in parties.iter() {
			if party.get_public_key().is_none() {
				panic!("No public key for party {}", party.party_index)
			}
		}
	}

	fn check_all_reached_offline_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		check_all_reached_stage(parties, Stage::OfflineReady)
	}

	fn check_all_reached_manual_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		let round_key = 1u32.encode();
		for party in parties.iter() {
			if !party.is_ready_to_vote(round_key.clone()) {
				return false
			}
		}
		true
	}

	fn check_all_signatures_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		for party in parties.iter() {
			if !party.is_signer() {
				continue
			}
			if !party.has_finished_rounds() {
				return false
			}
		}
		true
	}

	fn check_all_signatures_correct(parties: &mut Vec<MultiPartyECDSARounds<u32>>) {
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

		println!("All signatures are correct");
	}

	fn run_simulation<C>(parties: &mut Vec<MultiPartyECDSARounds<u32>>, stop_condition: C)
	where
		C: Fn(&Vec<MultiPartyECDSARounds<u32>>) -> bool,
	{
		println!("Simulation starts");

		let mut msgs_pull = vec![];

		for party in &mut parties.into_iter() {
			party.proceed(0);

			msgs_pull.append(&mut party.get_outgoing_messages());
		}

		for _i in 1..100 {
			let msgs_pull_frozen = msgs_pull.split_off(0);

			for party in &mut parties.into_iter() {
				for msg_frozen in msgs_pull_frozen.iter() {
					match party.handle_incoming(msg_frozen.clone(), None) {
						Ok(()) => (),
						Err(err) => panic!("{:?}", err),
					}
				}
				msgs_pull.append(&mut party.get_outgoing_messages());
			}

			for party in &mut parties.into_iter() {
				party.proceed(0);

				msgs_pull.append(&mut party.get_outgoing_messages());
			}

			if stop_condition(parties) {
				println!("All parties finished");
				return
			}
		}
	}

	fn simulate_multi_party(t: u16, n: u16) {
		let mut parties: Vec<MultiPartyECDSARounds<u32>> = vec![];
		let round_key = 1u32.encode();
		for i in 1..=n {
			let mut party = MultiPartyECDSARounds::new(i, t, n, 0, None, 0, None, None);
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
			println!("Creating offline stage");
			match party.create_offline_stage(round_key.clone(), 0) {
				Ok(()) => (),
				Err(_err) => (),
			}
		}
		run_simulation(&mut parties, check_all_reached_manual_ready);

		// Running Sign stage
		println!("Running Sign");
		let parties_refs = &mut parties;
		for party in &mut parties_refs.into_iter() {
			println!("Vote for party {}", party.party_index);
			party.vote(round_key.clone(), "Webb".encode(), 0).unwrap();
		}
		run_simulation(&mut parties, check_all_signatures_ready);

		// Extract all signatures and check for correctness
		check_all_signatures_correct(&mut parties);
	}

	#[test]
	fn simulate_multi_party_t2_n3() {
		simulate_multi_party(2, 3);
	}

	#[test]
	fn simulate_multi_party_t3_n5() {
		simulate_multi_party(3, 5);
	}
}
