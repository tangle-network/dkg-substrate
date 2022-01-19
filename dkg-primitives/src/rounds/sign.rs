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

/// Sign state

pub enum SignState<C> {
	NotStarted(PreSignRounds<C>),
	Started(SignRounds<C>),
	Finished(Result<DKGSignedPayload, DKGError>),
}

impl<C> DKGRoundsSM<DKGSignMessage, SignState<C>, C> for SignState<C> {
	fn proceed(&mut self, at: Clock) -> Result<bool, DKGError> {
		match self {
			Started(sign_rounds) => sign_rounds.proceed(at),
			_ => Ok(true)
		}
	}

	fn get_outgoing(&mut self) -> Vec<Payload> {
		match self {
			Started(sign_rounds) => sign_rounds.get_outgoing(),
			_ => vec![]
		}
	}

	fn handle_incoming(&mut self, data: Payload) -> Result<(), DKGError> {
		match self {
			Started(sign_rounds) => sign_rounds.handle_incoming(),
			_ => Ok(())
		}
	}

	fn is_finished(&self) -> bool {
		match self {
			Started(sign_rounds) => sign_rounds.is_finished(),
			_ => Ok(true)
		}
	}

	fn try_finish(self) -> Result<Self, DKGError> {
		match self {
			Started(sign_rounds) => {
				if sign_rounds.is_finished() {
					self
				} else {
					DKGError::SMNotFinished
				}
			},
			_ => self
		}
	}
}

/// Pre-sign rounds

pub struct PreSignRounds<Clock> {
	signer_set_id: SignerSetId,
	pending_sign_msgs: Vec<DKGSignMessage>,
}

impl<C> PreSignRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn new(signer_set_id: SignerSetId) -> Self {
		Self{
			signer_set_id,
			pending_sign_msgs: Vec::default(),
		}
	}
}

impl<C> DKGRoundsSM<DKGSignMessage, Vec<DKGSignMessage>, C> for PreSignRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn handle_incoming(&mut self, data: DKGSignMessage) -> Result<(), DKGError> {
		self.pending_sign_msgs.push(data);
		Ok(())
	}

	pub fn is_finished(&self) {
		true
	}
	
	pub fn try_finish(self) -> Result<Vec<DKGSignMessage>, DKGError> {
		Ok(self.pending_sign_msgs.take())
	}
}

/// Sign rounds

pub struct SignRounds<Clock> {
	params: SignParams,
	started_at: Clock,
	payload: Vec<u8>,
	round_key: Vec<u8>,
	partial_sig: PartialSignature,
	round_tracker: DKGRoundTracker<Vec<u8>, Clock>,
	sign_outgoing_msgs: Vec<DKGVoteMessage>,
}

impl<C> SignRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	pub fn new(
		params: SignParams,
		started_at: Clock,
		payload: Vec<u8>,
		round_key: Vec<u8>,
		partial_sig: PartialSignature,
		sign_manual: SignManual,
	) -> Self {
		let round_tracker = DKGRoundTracker::default();
		round_tracker.sign_manual = Some(sign_manual);
		round_tracker.payload = Some(payload);
		round_tracker.started_at = started_at;

		let mut sign_outgoing_msgs: Vec<DKGVoteMessage> = Vec::new();
		let serialized = serde_json::to_string(&partial_sig).unwrap();
		let msg = DKGVoteMessage {
			party_ind: params.party_index,
			round_key: round_key.clone(),
			partial_signature: serialized.into_bytes(),
		};
		sign_outgoing_msgs.push(msg);

		Self {
			params,
			started_at,
			payload,
			round_key,
			partial_sig,
			rounds,
			sign_outgoing_msgs,
		}
	}
}

impl<C> DKGRoundsSM<DKGVoteMessage, DKGSignedPayload, C> for SignRounds<C>
where
	C: AtLeast32BitUnsigned + Copy,
{
	/// Proceed to next step for current Stage

	pub fn proceed(&mut self, at: C) -> Result<bool, DKGError> {
		if let Err(err) = self.try_finish_vote() {
			return Err(err)
		} else {
			let mut timed_out = Vec::new();

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

	pub fn try_finish(&mut self) -> Result<bool, DKGError> {
		let mut finished = Vec::new();

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
					println!("{:?}", err);
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

							trace!(target: "dkg", "🕸️  Finished round /w key: {:?}", round_key);
							self.local_stages.remove(round_key);
						},
						_ => debug!("Error serializing signature"),
					}
				}
			}
		}

		Ok(false)
	}

	/// Get outgoing messages for current Stage

	pub fn get_outgoing(&mut self) -> Vec<DKGVoteMessage> {
		trace!(target: "dkg", "🕸️  Getting outgoing vote messages");
		std::mem::take(&mut self.sign_outgoing_msgs)
	}

	/// Handle incoming messages for current Stage

	pub fn handle_incoming(&mut self, data: DKGVoteMessage) -> Result<(), DKGError> {
		trace!(target: "dkg", "🕸️  Handle vote message");

		if data.party_ind == self.party_index {
			warn!(target: "dkg", "🕸️  Ignore messages sent by self");
			return Ok(())
		}

		let sig: PartialSignature = match serde_json::from_slice(&data.partial_signature) {
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
