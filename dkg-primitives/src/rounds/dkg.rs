use curv::{arithmetic::Converter, elliptic::curves::Secp256k1, BigInt};
use log::{debug, error, info, trace, warn};

use sp_runtime::traits::AtLeast32BitUnsigned;
use std::{collections::HashMap, path::PathBuf};

use super::{keygen::*, offline::*, sign::*};
use std::mem;
use typed_builder::TypedBuilder;

use crate::{types::*, utils::select_random_set};
use dkg_runtime_primitives::{crypto::AuthorityId, keccak_256};

pub use gg_2020::{
	party_i::*,
	state_machine::{keygen::*, sign::*},
};
pub use multi_party_ecdsa::protocols::multi_party_ecdsa::{
	gg_2020,
	gg_2020::state_machine::{keygen as gg20_keygen, sign as gg20_sign, traits::RoundBlame},
};

/// DKG State tracker
pub struct DKGState<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub accepted: bool,
	pub epoch_is_over: bool,
	pub listening_for_pub_key: bool,
	pub listening_for_active_pub_key: bool,
	pub curr_dkg: Option<MultiPartyECDSARounds<C>>,
	pub past_dkg: Option<MultiPartyECDSARounds<C>>,
	pub created_offlinestage_at: HashMap<Vec<u8>, C>,
}

/// State machine structure for performing Keygen, Offline stage and Sign rounds
/// HashMap and BtreeMap keys are encoded formats of (ChainIdType<ChainId>, DKGPayloadKey)
/// Using DKGPayloadKey only will cause collisions when proposals with the same nonce but from
/// different chains are submitted
///
/// Each of KeygenState, OfflineState and SignState is an enum of several sub-states.
/// All of them implement DKGRoundsSM trait for conveniece, but otherwise allow to be
/// pattern-matched to access internal state defined in keygen.rs, offline.rs, sing.rs.
///
/// They all share similar pattern of enum cases, where:
/// 1. NotStarted - is meant to only collect incoming messages in case some other party
/// has started earlier than us and already sent some messages.
/// 2. Started - sub-state is essentially the wrapper around corresponding
/// multi-party-ecdsa's state machine (Keygen, OfflineStage, SignManual)
/// 3. Finished - final sub-state with either execution result or error.
/// 4. Empty - special case for Keygen is a workaround to avoid using Option for self.keygen.
#[derive(TypedBuilder)]
pub struct MultiPartyECDSARounds<Clock>
where
	Clock: AtLeast32BitUnsigned + Copy,
{
	round_id: RoundId,
	party_index: u16,
	threshold: u16,
	parties: u16,

	// Key generation
	#[builder(default=KeygenState::NotStarted(PreKeygenRounds::new()))]
	keygen: KeygenState<Clock>,
	#[builder(default = false)]
	has_stalled: bool,

	// Signing
	#[builder(default)]
	signers: Vec<u16>,
	#[builder(default)]
	offlines: HashMap<Vec<u8>, OfflineState<Clock>>,
	#[builder(default)]
	votes: HashMap<Vec<u8>, SignState<Clock>>,

	// File system storage and encryption
	#[builder(default)]
	local_key_path: Option<PathBuf>,

	// Reputations
	#[builder(default)]
	reputations: HashMap<AuthorityId, i64>,

	// Authorities
	#[builder(default)]
	authorities: Vec<AuthorityId>,
}

impl<C> MultiPartyECDSARounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn get_local_key_path(&self) -> Option<PathBuf> {
		self.local_key_path.clone()
	}

	pub fn set_local_key(&mut self, local_key: LocalKey<Secp256k1>) {
		self.keygen = KeygenState::Finished(Ok(local_key));
	}

	pub fn set_signers(&mut self, signers: Vec<u16>) {
		self.signers = signers;
	}

	pub fn is_signer(&self) -> bool {
		self.signers.contains(&self.party_index)
	}

	pub fn dkg_params(&self) -> (u16, u16, u16) {
		(self.party_index, self.threshold, self.parties)
	}

	pub fn get_public_key(&self) -> Option<GE> {
		match &self.keygen {
			KeygenState::Finished(Ok(local_key)) => Some(local_key.public_key().clone()),
			_ => None,
		}
	}

	pub fn get_id(&self) -> RoundId {
		self.round_id
	}

	/// A check to know if the protocol has stalled at the keygen stage,
	/// We take it that the protocol has stalled if keygen messages are not received from other
	/// peers after a certain interval And the keygen stage has not completed.
	pub fn has_stalled(&self) -> bool {
		self.has_stalled
	}

	/// State machine

	/// Tries to proceed and make state transition if necessary for:
	/// 1. KeygenState if keygen is still in progress
	/// 2. Every OfflineState in self.offlines map
	/// 3. Every SignState in self.votes map
	///
	/// If the keygen is finished, we extract the `local_key` and set its
	/// state to `KeygenState::Finished`. We decide on the signing set
	/// when the `local_key` is extracted.
	pub fn proceed(&mut self, at: C) -> Vec<Result<DKGResult, DKGError>> {
		debug!(target: "dkg",
			"🕸️  State before proceed:\n round_id: {:?}, signers: {:?}",
			&self.round_id, &self.signers);

		let mut results = vec![];

		let keygen_proceed_res = self.keygen.proceed(at);
		if keygen_proceed_res.is_err() {
			if let Err(DKGError::KeygenTimeout { bad_actors }) = &keygen_proceed_res {
				error!(target: "dkg", "🕸️  Keygen timeout: {:?}", bad_actors);
				self.has_stalled = true;
			}
			results.push(keygen_proceed_res.map(|_| DKGResult::Empty));
		} else {
			if self.keygen.is_finished() {
				let prev_state = mem::replace(&mut self.keygen, KeygenState::Empty);
				self.keygen = match prev_state {
					KeygenState::Started(rounds) => {
						let finish_result = rounds.try_finish();
						if let Ok(local_key) = &finish_result {
							// TODO: Understand setting signers more deeply
							self.generate_and_set_signers(local_key);
							debug!("Party {}, new signers: {:?}", self.party_index, &self.signers);

							results.push(Ok(DKGResult::KeygenFinished {
								round_id: self.round_id,
								local_key: local_key.clone(),
							}));
						}
						KeygenState::Finished(finish_result)
					},
					_ => prev_state,
				};
			}
		}

		let offline_keys = self.offlines.keys().cloned().collect::<Vec<_>>();
		for key in offline_keys {
			if let Some(mut offline) = self.offlines.remove(&key.clone()) {
				let res = offline.proceed(at);
				if res.is_err() {
					results.push(res.map(|_| DKGResult::Empty));
				}
				let next_state = if offline.is_finished() {
					match offline {
						OfflineState::Started(rounds) =>
							OfflineState::Finished(rounds.try_finish()),
						_ => offline,
					}
				} else {
					offline
				};
				self.offlines.insert(key.clone(), next_state);
			}
		}

		let vote_keys = self.votes.keys().cloned().collect::<Vec<_>>();
		for key in vote_keys {
			if let Some(mut vote) = self.votes.remove(&key.clone()) {
				let res = vote.proceed(at);
				if res.is_err() {
					results.push(res.map(|_| DKGResult::Empty));
				}
				let next_state = if vote.is_finished() {
					match vote {
						SignState::Started(rounds) => SignState::Finished(rounds.try_finish()),
						_ => vote,
					}
				} else {
					vote
				};
				self.votes.insert(key.clone(), next_state);
			}
		}

		results
	}

	/// Collects and returns outgoing messages from
	/// KeygenState, every OfflineState and every SignState.
	pub fn get_outgoing_messages(&mut self) -> Vec<DKGMsgPayload> {
		trace!(target: "dkg", "🕸️  Get outgoing messages");

		let mut all_messages: Vec<DKGMsgPayload> = self
			.keygen
			.get_outgoing()
			.into_iter()
			.map(|msg| DKGMsgPayload::Keygen(msg))
			.collect();

		let offline_messages = self
			.offlines
			.values_mut()
			.map(|s| s.get_outgoing())
			.fold(Vec::new(), |mut acc, x| {
				acc.extend(x);
				acc
			})
			.into_iter()
			.map(|msg| DKGMsgPayload::Offline(msg))
			.collect::<Vec<_>>();

		let vote_messages = self
			.votes
			.values_mut()
			.map(|s| s.get_outgoing())
			.fold(Vec::new(), |mut acc, x| {
				acc.extend(x);
				acc
			})
			.into_iter()
			.map(|msg| DKGMsgPayload::Vote(msg))
			.collect::<Vec<_>>();

		all_messages.extend_from_slice(&offline_messages[..]);
		all_messages.extend_from_slice(&vote_messages[..]);

		all_messages
	}

	/// Depending on the DKGMsgPayload type, dispatches the message to:
	/// 1. KeygenState
	/// 2. OfflineState with the key corresponding to the one in the payload
	/// 3. SignState with the key corresponding to the one in the payload
	///
	/// If no OfflineState or SignState with the key in the payload is present in the corresponding
	/// map, a new entry with initial state is created (OfflineState::NotStarted or
	/// SignState::NotStarted)
	pub fn handle_incoming(&mut self, data: DKGMsgPayload, at: Option<C>) -> Result<(), DKGError> {
		trace!(target: "dkg", "🕸️  Handle incoming");

		return match data {
			DKGMsgPayload::Keygen(msg) =>
				self.keygen.handle_incoming(msg, at.or(Some(0u32.into())).unwrap()),
			DKGMsgPayload::Offline(msg) => {
				let key = msg.key.clone();

				let offline = self
					.offlines
					.entry(key.clone())
					.or_insert_with(|| OfflineState::NotStarted(PreOfflineRounds::new()));

				let res = offline.handle_incoming(msg, at.or(Some(0u32.into())).unwrap());
				if let Err(DKGError::CriticalError { reason: _ }) = res.clone() {
					self.offlines.remove(&key);
				}
				res
			},
			DKGMsgPayload::Vote(msg) => {
				let key = msg.round_key.clone();
				// Get the `SignState` or create a new one for this vote (a threshold signature).
				let vote = self
					.votes
					.entry(key.clone())
					.or_insert_with(|| SignState::NotStarted(PreSignRounds::new()));

				let res = vote.handle_incoming(msg, at.or(Some(0u32.into())).unwrap());
				if let Err(DKGError::CriticalError { reason: _ }) = res.clone() {
					self.votes.remove(&key);
				}
				res
			},
			_ => Ok(()),
		}
	}

	/// Starts keygen process for the current party.
	/// All incoming keygen messages collected so far will be proccessed immediately.
	pub fn start_keygen(&mut self, started_at: C) -> Result<(), DKGError> {
		info!(
			target: "dkg",
			"🕸️  Starting new DKG w/ party_index {:?}, threshold {:?}, size {:?}",
			self.party_index,
			self.threshold,
			self.parties,
		);

		let keygen_params = self.keygen_params();

		self.keygen = match mem::replace(&mut self.keygen, KeygenState::Empty) {
			KeygenState::NotStarted(pre_keygen) => {
				match Keygen::new(self.party_index, self.threshold, self.parties) {
					Ok(new_keygen) => {
						let mut keygen = KeygenState::Started(KeygenRounds::new(
							keygen_params,
							started_at,
							new_keygen,
						));

						// Processing pending messages
						if let Ok(pending_keygen_msgs) = pre_keygen.try_finish() {
							for msg in pending_keygen_msgs.clone() {
								if let Err(err) = keygen.handle_incoming(msg, started_at) {
									warn!(target: "dkg", "🕸️  Error handling pending keygen msg {:?}", err);
								}
								keygen.proceed(started_at)?;
							}
							trace!(target: "dkg", "🕸️  Handled {} pending keygen messages", &pending_keygen_msgs.len());
						}
						keygen
					},
					Err(err) => return Err(DKGError::StartKeygen { reason: err.to_string() }),
				}
			},
			_ => return Err(DKGError::StartKeygen { reason: "Already started".to_string() }),
		};

		Ok(())
	}

	/// Starts new offline stage for the provided key.
	/// All of the messages collected so far for this key will be processed immediately.
	pub fn create_offline_stage(&mut self, key: Vec<u8>, started_at: C) -> Result<(), DKGError> {
		debug!(target: "dkg", "🕸️  Creating offline stage for {:?} with signers {:?}", &key, &self.signers);

		let sign_params = self.sign_params();
		// Get the offline index in the signer set (different than the party index).
		let offline_i = match self.get_offline_stage_index() {
			Some(i) => i,
			None => {
				trace!(target: "dkg", "🕸️  We are not among signers, skipping");
				return Ok(())
			},
		};

		match &self.keygen {
			KeygenState::Finished(Ok(local_key)) =>
				return match OfflineStage::new(offline_i, self.signers.clone(), local_key.clone()) {
					Ok(new_offline_stage) => {
						let offline = self
							.offlines
							.entry(key.clone())
							.or_insert_with(|| OfflineState::NotStarted(PreOfflineRounds::new()));

						match offline {
							OfflineState::NotStarted(pre_offline) => {
								let mut new_offline = OfflineState::Started(OfflineRounds::new(
									sign_params,
									started_at,
									key.clone(),
									new_offline_stage,
								));

								for msg in pre_offline.pending_offline_msgs.clone() {
									if let Err(err) = new_offline.handle_incoming(msg, started_at) {
										warn!(target: "dkg", "🕸️  Error handling pending offline msg {:?}", err);
									}
									new_offline.proceed(started_at)?;
								}
								trace!(target: "dkg", "🕸️  Handled pending offline messages for {:?}", key);

								self.offlines.insert(key.clone(), new_offline);

								Ok(())
							},
							_ => Err(DKGError::CreateOfflineStage {
								reason: "Already started".to_string(),
							}),
						}
					},
					Err(err) => {
						error!("Error creating new offline stage {}", err);
						Err(DKGError::CreateOfflineStage { reason: err.to_string() })
					},
				},
			_ => Err(DKGError::CreateOfflineStage {
				reason: "Cannot start offline stage, Keygen is not complete".to_string(),
			}),
		}
	}

	/// Starts new signing process for the provided key.
	/// All of the messages collected so far for this key will be processed immediately.
	pub fn vote(
		&mut self,
		round_key: Vec<u8>,
		data: Vec<u8>,
		started_at: C,
	) -> Result<(), DKGError> {
		if let Some(OfflineState::Finished(Ok(completed_offline))) =
			self.offlines.remove(&round_key)
		{
			let hash = BigInt::from_bytes(&keccak_256(&data));

			let sign_params = self.sign_params();

			match SignManual::new(hash, completed_offline.clone()) {
				Ok((sign_manual, sig)) => {
					debug!(target: "dkg", "🕸️  Creating vote w/ key {:?}", &round_key);

					let vote = self
						.votes
						.entry(round_key.clone())
						.or_insert_with(|| SignState::NotStarted(PreSignRounds::new()));

					match vote {
						SignState::NotStarted(pre_sign) => {
							let mut new_sign = SignState::Started(SignRounds::new(
								sign_params,
								started_at,
								data,
								round_key.clone(),
								sig,
								sign_manual,
							));

							for msg in pre_sign.pending_sign_msgs.clone() {
								if let Err(err) = new_sign.handle_incoming(msg, started_at) {
									warn!(target: "dkg", "🕸️  Error handling pending vote msg {:?}", err);
								}
								new_sign.proceed(started_at)?;
							}
							debug!(target: "dkg", "🕸️  Handled pending vote messages for {:?}", round_key);

							self.votes.insert(round_key.clone(), new_sign);

							Ok(())
						},
						_ => Err(DKGError::Vote { reason: "Already started".to_string() }),
					}
				},
				Err(err) => Err(DKGError::Vote { reason: err.to_string() }),
			}
		} else {
			Err(DKGError::Vote { reason: "Not ready to vote".to_string() })
		}
	}

	pub fn is_keygen_in_progress(&self) -> bool {
		match self.keygen {
			KeygenState::Finished(_) => false,
			_ => true,
		}
	}

	pub fn is_keygen_finished(&self) -> bool {
		match self.keygen {
			KeygenState::Finished(_) => true,
			_ => false,
		}
	}

	pub fn is_ready_to_vote(&self, round_key: Vec<u8>) -> bool {
		if let Some(offline) = self.offlines.get(&round_key) {
			match offline {
				OfflineState::Finished(Ok(_)) => true,
				_ => false,
			}
		} else {
			false
		}
	}

	pub fn has_vote_in_process(&self, round_key: Vec<u8>) -> bool {
		return self.votes.contains_key(&round_key)
	}

	pub fn has_finished_rounds(&self) -> bool {
		let finished = self
			.votes
			.values()
			.filter(|v| match v {
				SignState::Finished(_) => true,
				_ => false,
			})
			.count();

		finished > 0
	}

	pub fn get_finished_rounds(&mut self) -> Vec<DKGSignedPayload> {
		let mut finished: Vec<DKGSignedPayload> = vec![];

		let vote_keys = self.votes.keys().cloned().collect::<Vec<_>>();
		for key in vote_keys {
			if let Some(vote) = self.votes.remove(&key.clone()) {
				if vote.is_finished() {
					match vote {
						SignState::Finished(Ok(signed_payload)) => {
							finished.push(signed_payload);
						},
						_ => {
							self.votes.insert(key.clone(), vote);
						},
					}
				} else {
					self.votes.insert(key.clone(), vote);
				};
			}
		}

		finished
	}

	/// Utils

	/// Returns our party's index in signers vec if any.
	/// Indexing starts from 1.
	/// OfflineStage must be created using this index if present (not the original keygen index)
	fn get_offline_stage_index(&self) -> Option<u16> {
		for (i, &keygen_i) in (1..).zip(&self.signers) {
			if self.party_index == keygen_i {
				return Some(i)
			}
		}
		None
	}

	/// Generates the signer set by randomly selecting t+1 signers
	/// to participate in the signing protocol. We set the signers in the local
	/// storage once selected.
	fn generate_and_set_signers(&mut self, local_key: &LocalKey<Secp256k1>) {
		let (_, threshold, _parties) = self.dkg_params();
		let seed = &local_key.clone().public_key().to_bytes(true)[1..];
		// Get the parties with non-negative reputation
		let good_parties: Vec<u16> = self
			.authorities
			.iter()
			.enumerate()
			.filter(|(_, a)| self.reputations.get(a).unwrap_or(&0i64) >= &0i64)
			.map(|(index, _)| (index + 1) as u16)
			.collect();
		// If there aren't enough good authorities
		let signers_set: Result<Vec<u16>, &str>;
		if good_parties.len() <= threshold as usize {
			// Get bad party indices and their reputations. Bad parties are those with negative
			// reputation.
			let mut bad_parties_and_reps: Vec<(u16, i64)> = self
				.authorities
				.iter()
				.enumerate()
				.filter(|(_index, a)| self.reputations.get(a).unwrap_or(&0i64) < &0i64)
				.map(|(index, a)| (index + 1, *self.reputations.get(a).unwrap()))
				.map(|(index, rep)| ((index + 1) as u16, rep))
				.collect::<Vec<(u16, i64)>>();
			// Sort them in descending order
			bad_parties_and_reps.sort_by(|a, b| b.1.cmp(&a.1));
			// Get the best bad parties to fill `threshold + 1` slots
			let needed_bad_parties = bad_parties_and_reps
				.iter()
				.take((threshold as usize) + 1 - good_parties.len())
				.map(|k| k.0)
				.collect::<Vec<u16>>();
			// Join the good and bad parties to get the final signers set
			let good_and_bad_authorities: Vec<u16> =
				good_parties.iter().chain(needed_bad_parties.iter()).map(|i| *i).collect();
			signers_set = select_random_set(seed, good_and_bad_authorities, threshold + 1);
		} else {
			signers_set = select_random_set(seed, good_parties, threshold + 1);
		}

		if let Ok(signers_set) = signers_set {
			self.set_signers(signers_set);
		}
	}

	fn keygen_params(&self) -> KeygenParams {
		KeygenParams {
			round_id: self.round_id,
			party_index: self.party_index,
			threshold: self.threshold,
			parties: self.parties,
		}
	}

	fn sign_params(&self) -> SignParams {
		// TODO: Currently we use round_id as signer_set_id for consistency,
		// but eventually we want to derive this id from both round_id and signers vec
		SignParams {
			round_id: self.round_id,
			party_index: self.party_index,
			threshold: self.threshold,
			parties: self.parties,
			signers: self.signers.clone(),
			signer_set_id: self.round_id,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{KeygenState, MultiPartyECDSARounds};
	use codec::Encode;

	fn check_all_parties_have_public_key(parties: &Vec<MultiPartyECDSARounds<u32>>) {
		for party in parties.iter() {
			if party.get_public_key().is_none() {
				panic!("No public key for party {}", party.party_index)
			}
		}
	}

	fn check_all_reached_offline_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		for party in parties.iter() {
			match &party.keygen {
				KeygenState::Finished(_) => (),
				_ => return false,
			}
		}
		true
	}

	fn check_all_reached_manual_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		let round_key = 1u32.encode();
		for party in parties.iter() {
			if party.is_signer() && !party.is_ready_to_vote(round_key.clone()) {
				return false
			}
		}
		true
	}

	fn check_all_signatures_ready(parties: &Vec<MultiPartyECDSARounds<u32>>) -> bool {
		for party in parties.iter() {
			if party.is_signer() && !party.has_finished_rounds() {
				return false
			}
		}
		true
	}

	fn check_all_signatures_correct(parties: &mut Vec<MultiPartyECDSARounds<u32>>) {
		for party in &mut parties.into_iter() {
			if party.is_signer() {
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
			let proceed_res = party.proceed(0);
			for res in proceed_res {
				if let Err(err) = res {
					println!("Error: {:?}", err);
				}
			}

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
				let proceed_res = party.proceed(0);
				for res in proceed_res {
					if let Err(err) = res {
						println!("Error: {:?}", err);
					}
				}

				msgs_pull.append(&mut party.get_outgoing_messages());
			}

			if stop_condition(parties) {
				println!("All parties finished");
				return
			}
		}

		panic!("Not all parties finished");
	}

	fn simulate_multi_party(t: u16, n: u16) {
		let mut parties: Vec<MultiPartyECDSARounds<u32>> = vec![];
		let round_key = 1u32.encode();
		for i in 1..=n {
			let mut party = MultiPartyECDSARounds::builder()
				.round_id(0)
				.party_index(i)
				.threshold(t)
				.parties(n)
				.build();
			println!("Starting keygen for party {}", party.party_index);
			party.start_keygen(0).unwrap();
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
			match party.vote(round_key.clone(), "Webb".encode(), 0) {
				Ok(()) => (),
				Err(_err) => (),
			}
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
