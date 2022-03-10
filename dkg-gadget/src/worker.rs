// Copyright (C) 2020-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![allow(clippy::collapsible_match)]

use sc_keystore::LocalKeystore;
use std::{
	collections::{BTreeSet, HashMap},
	marker::PhantomData,
	path::PathBuf,
	sync::Arc,
};

use codec::{Codec, Decode, Encode};
use futures::{future, FutureExt, StreamExt};
use log::{debug, error, info, trace};
use parking_lot::Mutex;

use sc_client_api::{
	Backend, BlockImportNotification, FinalityNotification, FinalityNotifications,
	ImportNotifications,
};
use sc_network_gossip::GossipEngine;

use rand::Rng;
use sp_api::{
	offchain::{OffchainStorage, STORAGE_PREFIX},
	BlockId,
};
use sp_runtime::{
	traits::{Block, Header, NumberFor},
	AccountId32,
};

use crate::{
	keystore::DKGKeystore,
	persistence::{try_restart_dkg, try_resume_dkg, DKGPersistenceState},
};

use crate::messages::{
	dkg_message::send_outgoing_dkg_messages,
	misbehaviour_report::{gossip_misbehaviour_report, handle_misbehaviour_report},
	public_key_gossip::handle_public_key_broadcast,
};

use dkg_primitives::{
	types::{DKGError, RoundId},
	ChainId, DKGReport, Proposal,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	offchain::storage_keys::{
		AGGREGATED_MISBEHAVIOUR_REPORTS, AGGREGATED_PUBLIC_KEYS, AGGREGATED_PUBLIC_KEYS_AT_GENESIS,
		OFFCHAIN_PUBLIC_KEY_SIG, OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_GENESIS_KEYS_AT, SUBMIT_KEYS_AT,
	},
	utils::{sr25519, to_slice_32},
	AggregatedMisbehaviourReports, AggregatedPublicKeys, ChainIdType, OffchainSignedProposals,
	GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	error::{self},
	metric_inc, metric_set,
	metrics::Metrics,
	proposal::get_signed_proposal,
	types::dkg_topic,
	utils::{
		fetch_public_key, fetch_sr25519_public_key, find_authorities_change, find_index,
		get_key_path, is_next_authorities_or_rounds_empty, is_queued_authorities_or_rounds_empty,
		set_up_rounds, validate_threshold,
	},
	Client,
};

use dkg_primitives::{
	rounds::{DKGState, MultiPartyECDSARounds},
	types::{DKGMessage, DKGPayloadKey, DKGSignedPayload, SignedDKGMessage},
	utils::{cleanup, DKG_LOCAL_KEY_FILE, QUEUED_DKG_LOCAL_KEY_FILE},
};
use dkg_runtime_primitives::{AuthoritySet, DKGApi};

pub const ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

pub const STORAGE_SET_RETRY_NUM: usize = 5;

pub const MAX_SUBMISSION_DELAY: u32 = 3;

pub(crate) struct WorkerParams<B, BE, C>
where
	B: Block,
{
	pub client: Arc<C>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: GossipEngine<B>,
	pub metrics: Option<Metrics>,
	pub base_path: Option<PathBuf>,
	pub local_keystore: Option<Arc<LocalKeystore>>,
	pub dkg_state: DKGState<NumberFor<B>>,
}

/// A DKG worker plays the DKG protocol
pub(crate) struct DKGWorker<B, C, BE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
{
	pub client: Arc<C>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: Arc<Mutex<GossipEngine<B>>>,
	metrics: Option<Metrics>,
	pub rounds: Option<MultiPartyECDSARounds<NumberFor<B>>>,
	pub next_rounds: Option<MultiPartyECDSARounds<NumberFor<B>>>,
	finality_notifications: FinalityNotifications<B>,
	block_import_notification: ImportNotifications<B>,
	/// Best block we received a GRANDPA notification for
	best_grandpa_block: NumberFor<B>,
	/// Best block a DKG voting round has been concluded for
	best_dkg_block: Option<NumberFor<B>>,
	/// Latest block header
	pub latest_header: Option<B::Header>,
	/// Current validator set
	pub current_validator_set: AuthoritySet<Public>,
	/// Queued validator set
	pub queued_validator_set: AuthoritySet<Public>,
	/// Validator set id for the last signed commitment
	last_signed_id: u64,
	/// keep rustc happy
	_backend: PhantomData<BE>,
	/// public key refresh in progress
	refresh_in_progress: bool,
	/// Msg cache for startup if authorities aren't set
	msg_cache: Vec<SignedDKGMessage<AuthorityId>>,
	/// Tracking for the broadcasted public keys and signatures
	pub aggregated_public_keys: HashMap<RoundId, AggregatedPublicKeys>,
	/// Tracking for the misbehaviour reports
	pub aggregated_misbehaviour_reports:
		HashMap<(RoundId, AuthorityId), AggregatedMisbehaviourReports>,
	/// dkg state
	pub dkg_state: DKGState<NumberFor<B>>,
	/// Setting up keygen for current authorities
	pub active_keygen_in_progress: bool,
	/// setting up queued authorities keygen
	pub queued_keygen_in_progress: bool,
	/// Track DKG Persistence state
	pub dkg_persistence: DKGPersistenceState,
	/// Local keystore for DKG data
	pub base_path: Option<PathBuf>,
	/// Concrete type that points to the actual local keystore if it exists
	pub local_keystore: Option<Arc<LocalKeystore>>,
}

impl<B, C, BE> DKGWorker<B, C, BE>
where
	B: Block + Codec,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	/// Return a new DKG worker instance.
	///
	/// Note that a DKG worker is only fully functional if a corresponding
	/// DKG pallet has been deployed on-chain.
	///
	/// The DKG pallet is needed in order to keep track of the DKG authority set.
	pub(crate) fn new(worker_params: WorkerParams<B, BE, C>) -> Self {
		let WorkerParams {
			client,
			backend,
			key_store,
			gossip_engine,
			metrics,
			dkg_state,
			base_path,
			local_keystore,
		} = worker_params;

		DKGWorker {
			client: client.clone(),
			backend,
			key_store,
			gossip_engine: Arc::new(Mutex::new(gossip_engine)),
			metrics,
			rounds: None,
			next_rounds: None,
			finality_notifications: client.finality_notification_stream(),
			block_import_notification: client.import_notification_stream(),
			best_grandpa_block: client.info().finalized_number,
			best_dkg_block: None,
			current_validator_set: AuthoritySet::empty(),
			queued_validator_set: AuthoritySet::empty(),
			latest_header: None,
			last_signed_id: 0,
			dkg_state,
			queued_keygen_in_progress: false,
			active_keygen_in_progress: false,
			refresh_in_progress: false,
			msg_cache: Vec::new(),
			aggregated_public_keys: HashMap::new(),
			aggregated_misbehaviour_reports: HashMap::new(),
			dkg_persistence: DKGPersistenceState::new(),
			base_path,
			local_keystore,
			_backend: PhantomData,
		}
	}

	/// gets the current validators in the authority set
	pub fn get_current_validators(&self) -> AuthoritySet<AuthorityId> {
		self.current_validator_set.clone()
	}

	/// gets the queued(next) validators in the authority set
	pub fn get_queued_validators(&self) -> AuthoritySet<AuthorityId> {
		self.queued_validator_set.clone()
	}

	/// gets the dkg keystore(cryto keys)
	pub fn keystore_ref(&self) -> DKGKeystore {
		self.key_store.clone()
	}

	/// set the current rounds
	pub fn set_rounds(&mut self, rounds: MultiPartyECDSARounds<NumberFor<B>>) {
		self.rounds = Some(rounds);
	}

	/// sets the next rounds
	pub fn set_next_rounds(&mut self, rounds: MultiPartyECDSARounds<NumberFor<B>>) {
		self.next_rounds = Some(rounds);
	}

	/// gets the current rounds
	pub fn take_rounds(&mut self) -> Option<MultiPartyECDSARounds<NumberFor<B>>> {
		self.rounds.take()
	}

	/// gets the next rounds
	pub fn take_next_rounds(&mut self) -> Option<MultiPartyECDSARounds<NumberFor<B>>> {
		self.next_rounds.take()
	}
}

impl<B, C, BE> DKGWorker<B, C, BE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	/// returns the index of an authority from an header
	fn _get_authority_index(&mut self, header: &B::Header) -> Option<usize> {
		let new = if let Some((new, ..)) = find_authorities_change::<B>(header) {
			Some(new)
		} else {
			let at: BlockId<B> = BlockId::hash(header.hash());
			self.client.runtime_api().authority_set(&at).ok()
		};

		trace!(target: "dkg", "🕸️  active validator set: {:?}", new);

		let set = new.unwrap_or_else(|| panic!("Help"));
		let public = fetch_public_key(self);
		for i in 0..set.authorities.len() {
			if set.authorities[i] == public {
				return Some(i)
			}
		}

		return None
	}

	/// gets authority reputations from an header
	pub fn get_authority_reputations(&self, header: &B::Header) -> HashMap<AuthorityId, i64> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		let reputations = self
			.client
			.runtime_api()
			.get_reputations(&at, self.current_validator_set.authorities.clone())
			.expect("get reputations is always available");
		let mut reputation_map: HashMap<AuthorityId, i64> = HashMap::new();
		for (id, rep) in reputations {
			reputation_map.insert(id, rep);
		}

		reputation_map
	}

	/// get the signature threshold of a block in an header
	pub fn get_threshold(&self, header: &B::Header) -> Option<u16> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().signature_threshold(&at).ok()
	}

	pub fn get_time_to_restart(&self, header: &B::Header) -> Option<NumberFor<B>> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().time_to_restart(&at).ok()
	}

	/// gets latest block number from latest block header
	pub fn get_latest_block_number(&self) -> NumberFor<B> {
		self.latest_header.clone().unwrap().number().clone()
	}

	/// Return the next and queued validator set at header `header`.
	///
	/// Note that the validator set could be `None`. This is the case if we don't find
	/// a DKG authority set change and we can't fetch the authority set from the
	/// DKG on-chain state.
	///
	/// Such a failure is usually an indication that the DKG pallet has not been deployed (yet).
	///
	/// If the validators are None, we use the arbitrary validators gotten from the authority set
	/// and queued authority set in the given header
	pub fn validator_set(
		&self,
		header: &B::Header,
	) -> Option<(AuthoritySet<Public>, AuthoritySet<Public>)> {
		let new = if let Some((new, queued)) = find_authorities_change::<B>(header) {
			Some((new, queued))
		} else {
			let at: BlockId<B> = BlockId::hash(header.hash());
			Some((
				self.client.runtime_api().authority_set(&at).ok().unwrap_or_default(),
				self.client.runtime_api().queued_authority_set(&at).ok().unwrap_or_default(),
			))
		};

		trace!(target: "dkg", "🕸️  active validator set: {:?}", new);

		new
	}

	/// Verify `active` validator set for `block` against the key store
	///
	/// The critical case is, if we do have a public key in the key store which is not
	/// part of the active validator set.
	///
	/// Note that for a non-authority node there will be no keystore, and we will
	/// return an error and don't check. The error can usually be ignored.
	fn verify_validator_set(
		&self,
		block: &NumberFor<B>,
		mut active: AuthoritySet<Public>,
	) -> Result<(), error::Error> {
		let active: BTreeSet<Public> = active.authorities.drain(..).collect();

		let store: BTreeSet<Public> = self.key_store.public_keys()?.drain(..).collect();

		let missing: Vec<_> = store.difference(&active).cloned().collect();

		if !missing.is_empty() {
			debug!(target: "dkg", "🕸️  for block {:?}, public key missing in validator set is: {:?}", block, missing);
		}

		Ok(())
	}

	fn handle_dkg_setup(&mut self, header: &B::Header, next_authorities: AuthoritySet<Public>) {
		if is_next_authorities_or_rounds_empty(self, &next_authorities) {
			return
		}

		let public = fetch_public_key(self);
		let sr25519_public = fetch_sr25519_public_key(self);

		let thresh = validate_threshold(
			next_authorities.authorities.len() as u16,
			self.get_threshold(header).unwrap(),
		);

		let mut local_key_path = None;
		let mut queued_local_key_path = None;

		if self.base_path.is_some() {
			local_key_path = get_key_path(&self.base_path, DKG_LOCAL_KEY_FILE);
			queued_local_key_path = get_key_path(&self.base_path, QUEUED_DKG_LOCAL_KEY_FILE);
			let _ = cleanup(local_key_path.as_ref().unwrap().clone());
		}

		let latest_block_num = self.get_latest_block_number();

		self.handle_setting_of_rounds(
			&next_authorities,
			public,
			sr25519_public,
			local_key_path,
			queued_local_key_path,
			header,
			thresh,
		);

		if next_authorities.id == GENESIS_AUTHORITY_SET_ID {
			self.dkg_state.listening_for_active_pub_key = true;

			match self.rounds.as_mut().unwrap().start_keygen(latest_block_num) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for genesis authority set successfully");
					self.active_keygen_in_progress = true;
				},
				Err(err) => {
					error!("Error starting keygen {:?}", &err);
					self.handle_dkg_error(err);
				},
			}
		}
	}

	/// sets the current rounds if there is next rounds in the Dkg worker instance
	///
	/// else it creates new rounds to set
	fn handle_setting_of_rounds(
		&mut self,
		next_authorities: &AuthoritySet<Public>,
		public: Public,
		sr25519_public: sp_application_crypto::sr25519::Public,
		local_key_path: Option<PathBuf>,
		queued_local_key_path: Option<PathBuf>,
		header: &B::Header,
		thresh: u16,
	) {
		self.rounds = if self.next_rounds.is_some() {
			if let (Some(path), Some(queued_path)) = (local_key_path, queued_local_key_path) {
				if let Err(err) = std::fs::copy(queued_path, path) {
					error!("Error copying queued key {:?}", &err);
				} else {
					debug!("Successfully copied queued key to current key");
				}
			}
			self.next_rounds.take()
		} else {
			let is_authority = find_index(&next_authorities.authorities[..], &public).is_some();
			debug!(target: "dkg", "🕸️  public: {:?} is_authority: {:?}", public, is_authority);
			if next_authorities.id == GENESIS_AUTHORITY_SET_ID && is_authority {
				Some(set_up_rounds(
					&next_authorities,
					&public,
					&sr25519_public,
					thresh,
					local_key_path,
					*header.number(),
					self.local_keystore.clone(),
					&self.get_authority_reputations(header),
				))
			} else {
				None
			}
		};
	}

	fn handle_queued_dkg_setup(&mut self, header: &B::Header, queued: AuthoritySet<Public>) {
		if is_queued_authorities_or_rounds_empty(self, &queued) {
			return
		}

		let public = fetch_public_key(self);
		let sr25519_public = fetch_sr25519_public_key(self);

		let thresh = validate_threshold(
			queued.authorities.len() as u16,
			self.get_threshold(header).unwrap_or_default(),
		);

		let mut local_key_path = None;

		if self.base_path.is_some() {
			local_key_path = get_key_path(&self.base_path, QUEUED_DKG_LOCAL_KEY_FILE);
			let _ = cleanup(local_key_path.as_ref().unwrap().clone());
		}
		let latest_block_num = self.get_latest_block_number();

		// If current node is part of the queued authorities
		// start the multiparty keygen process
		if queued.authorities.contains(&public) {
			// Setting up DKG for queued authorities
			self.next_rounds = Some(set_up_rounds(
				&queued,
				&public,
				&sr25519_public,
				thresh,
				local_key_path,
				*header.number(),
				self.local_keystore.clone(),
				&self.get_authority_reputations(header),
			));
			self.dkg_state.listening_for_pub_key = true;
			match self.next_rounds.as_mut().unwrap().start_keygen(latest_block_num) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for queued authority set successfully");
					self.queued_keygen_in_progress = true;
				},
				Err(err) => {
					error!("Error starting keygen {:?}", &err);
					self.handle_dkg_error(err);
				},
			}
		}
	}

	// *** Block notifications ***
	fn process_block_notification(&mut self, header: &B::Header) {
		if let Some(latest_header) = &self.latest_header {
			if latest_header.number() >= header.number() {
				return
			}
		}

		self.latest_header = Some(header.clone());
		self.listen_and_clear_offchain_storage(header);
		try_resume_dkg(self, header);

		self.enact_new_authorities(header);

		try_restart_dkg(self, header);
		send_outgoing_dkg_messages(self);
		self.create_offline_stages(header);
		self.process_unsigned_proposals(header);
		self.untrack_unsigned_proposals(header);
	}

	fn enact_new_authorities(&mut self, header: &B::Header) {
		if let Some((active, queued)) = self.validator_set(header) {
			// Authority set change or genesis set id triggers new voting rounds
			//
			// TODO: Enacting a new authority set will also implicitly 'conclude'
			//		 the currently active DKG voting round by starting a new one.
			if active.id != self.current_validator_set.id ||
				(active.id == GENESIS_AUTHORITY_SET_ID && self.best_dkg_block.is_none())
			{
				debug!(target: "dkg", "🕸️  New active validator set id: {:?}", active);
				metric_set!(self, dkg_validator_set_id, active.id);

				// DKG should produce a signed commitment for each session
				if active.id != self.last_signed_id + 1 && active.id != GENESIS_AUTHORITY_SET_ID {
					metric_inc!(self, dkg_skipped_sessions);
				}

				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				// Setting new validator set id as curent
				self.current_validator_set = active.clone();
				self.queued_validator_set = queued.clone();

				debug!(target: "dkg", "🕸️  New Rounds for id: {:?}", active.id);

				self.best_dkg_block = Some(*header.number());

				// this metric is kind of 'fake'. Best DKG block should only be updated once we have
				// a signed commitment for the block. Remove once the above TODO is done.
				metric_set!(self, dkg_best_block, *header.number());
				debug!(target: "dkg", "🕸️  Active validator set {:?}: {:?}", active.id, active.clone().authorities.iter().map(|x| format!("\n{:?}", x)).collect::<Vec<String>>());
				debug!(target: "dkg", "🕸️  Queued validator set {:?}: {:?}", queued.id, queued.clone().authorities.iter().map(|x| format!("\n{:?}", x)).collect::<Vec<String>>());
				// Setting up the DKG
				self.handle_dkg_setup(&header, active.clone());
				self.handle_queued_dkg_setup(&header, queued.clone());

				if !self.current_validator_set.authorities.is_empty() {
					send_outgoing_dkg_messages(self);
				}
				self.dkg_state.epoch_is_over = !self.dkg_state.epoch_is_over;
			} else {
				// if the DKG has not been prepared / terminated, continue preparing it
				if !self.dkg_state.accepted || self.queued_keygen_in_progress {
					send_outgoing_dkg_messages(self);
				}
			}

			if self.queued_validator_set.authorities != queued.authorities &&
				!self.queued_keygen_in_progress
			{
				debug!(target: "dkg", "🕸️  Queued authorities changed, running key gen");
				self.queued_validator_set = queued.clone();
				self.handle_queued_dkg_setup(&header, queued.clone());
				send_outgoing_dkg_messages(self);
			}
		}
	}

	fn handle_import_notifications(&mut self, notification: BlockImportNotification<B>) {
		trace!(target: "dkg", "🕸️  Block import notification: {:?}", notification);
		self.process_block_notification(&notification.header);
	}

	fn handle_finality_notification(&mut self, notification: FinalityNotification<B>) {
		trace!(target: "dkg", "🕸️  Finality notification: {:?}", notification);

		// update best GRANDPA finalized block we have seen
		self.best_grandpa_block = *notification.header.number();

		self.process_block_notification(&notification.header);
	}

	fn verify_signature_against_authorities(
		&mut self,
		signed_dkg_msg: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let dkg_msg = signed_dkg_msg.msg;
		let encoded = dkg_msg.encode();
		let signature = signed_dkg_msg.signature.unwrap_or_default();
		// Get authority accounts
		let mut authority_accounts: Option<(Vec<AccountId32>, Vec<AccountId32>)> = None;

		if let Some(header) = self.latest_header.as_ref() {
			let at: BlockId<B> = BlockId::hash(header.hash());
			let accounts = self.client.runtime_api().get_authority_accounts(&at).ok();

			if accounts.is_some() {
				authority_accounts = accounts;
			}
		}

		if authority_accounts.is_none() {
			return Err(DKGError::GenericError { reason: "No authorities".into() })
		}

		let check_signers = |xs: Vec<AccountId32>| {
			return dkg_runtime_primitives::utils::verify_signer_from_set(
				xs.iter()
					.map(|x| {
						sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
							panic!("Failed to convert account id to sr25519 public key")
						}))
					})
					.collect(),
				&encoded,
				&signature,
			)
			.1
		};

		if check_signers(authority_accounts.clone().unwrap().0.into()) ||
			check_signers(authority_accounts.clone().unwrap().1.into())
		{
			return Ok(dkg_msg)
		} else {
			return Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority or next authority"
					.into(),
			})
		}
	}

	fn process_incoming_dkg_message(&mut self, dkg_msg: DKGMessage<Public>) {
		if let Some(rounds) = self.rounds.as_mut() {
			if dkg_msg.round_id == rounds.get_id() {
				let block_number = {
					if self.latest_header.is_some() {
						Some(*self.latest_header.as_ref().unwrap().number())
					} else {
						None
					}
				};
				debug!(target: "dkg", "🕸️  Process DKG message for current rounds {}", &dkg_msg);
				match rounds.handle_incoming(dkg_msg.payload.clone(), block_number) {
					Ok(()) => (),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}

				if rounds.is_keygen_finished() {
					debug!(target: "dkg", "🕸️  DKG is ready to sign");
					self.dkg_state.accepted = true;
				}
			}
		}

		if let Some(next_rounds) = self.next_rounds.as_mut() {
			if next_rounds.get_id() == dkg_msg.round_id {
				let block_number = {
					if self.latest_header.is_some() {
						Some(*self.latest_header.as_ref().unwrap().number())
					} else {
						None
					}
				};

				debug!(target: "dkg", "🕸️  Process DKG message for queued rounds {}", &dkg_msg);
				match next_rounds.handle_incoming(dkg_msg.payload.clone(), block_number) {
					Ok(()) => debug!(target: "dkg", "🕸️  Handled incoming messages"),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}
			}
		}

		match handle_public_key_broadcast(self, dkg_msg.clone()) {
			Ok(()) => (),
			Err(err) => debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
		};

		match handle_misbehaviour_report(self, dkg_msg.clone()) {
			Ok(()) => (),
			Err(err) => debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
		};

		send_outgoing_dkg_messages(self);
		self.process_finished_rounds();
	}

	pub fn handle_dkg_error(&mut self, dkg_error: DKGError) {
		let authorities = self.current_validator_set.authorities.clone();

		let bad_actors = match dkg_error {
			DKGError::KeygenMisbehaviour { ref bad_actors } => bad_actors.clone(),
			DKGError::KeygenTimeout { ref bad_actors } => bad_actors.clone(),
			DKGError::OfflineMisbehaviour { ref bad_actors } => bad_actors.clone(),
			DKGError::OfflineTimeout { ref bad_actors } => bad_actors.clone(),
			DKGError::SignMisbehaviour { ref bad_actors } => bad_actors.clone(),
			DKGError::SignTimeout { ref bad_actors } => bad_actors.clone(),
			_ => Default::default(),
		};

		let mut offenders: Vec<AuthorityId> = Vec::new();
		for bad_actor in bad_actors {
			let bad_actor = bad_actor as usize;
			if bad_actor > 0 && bad_actor <= authorities.len() {
				if let Some(offender) = authorities.get(bad_actor - 1) {
					offenders.push(offender.clone());
				}
			}
		}

		for offender in offenders {
			match dkg_error {
				DKGError::KeygenMisbehaviour { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender }),
				DKGError::KeygenTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender }),
				DKGError::OfflineMisbehaviour { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehaviour { offender }),
				DKGError::OfflineTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehaviour { offender }),
				DKGError::SignMisbehaviour { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehaviour { offender }),
				DKGError::SignTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehaviour { offender }),
				_ => (),
			}
		}
	}

	fn handle_dkg_report(&mut self, dkg_report: DKGReport) {
		let round_id = if let Some(rounds) = &self.rounds { rounds.get_id() } else { 0 };

		match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender } => {
				debug!(target: "dkg", "🕸️  DKG Keygen misbehaviour by {}", offender);
				gossip_misbehaviour_report(self, offender, round_id);
			},
			DKGReport::SigningMisbehaviour { offender } => {
				debug!(target: "dkg", "🕸️  DKG Signing misbehaviour by {}", offender);
				gossip_misbehaviour_report(self, offender, round_id);
			},
		}
	}

	/// Offchain features
	fn listen_and_clear_offchain_storage(&mut self, header: &B::Header) {
		let at: BlockId<B> = BlockId::hash(header.hash());
		let next_dkg_public_key = self.client.runtime_api().next_dkg_pub_key(&at);
		let dkg_public_key = self.client.runtime_api().dkg_pub_key(&at);
		let public_key_sig = self.client.runtime_api().next_pub_key_sig(&at);

		let offchain = self.backend.offchain_storage();

		if let Some(mut offchain) = offchain {
			if let Ok(Some(_key)) = next_dkg_public_key {
				if offchain.get(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS).is_some() {
					debug!(target: "dkg", "cleaned offchain storage, next_public_key: {:?}", _key);
					offchain.remove(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS);

					offchain.remove(STORAGE_PREFIX, SUBMIT_KEYS_AT);
				}
			}

			if let Ok(Some(_key)) = dkg_public_key {
				if offchain.get(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS_AT_GENESIS).is_some() {
					debug!(target: "dkg", "cleaned offchain storage, genesis_pub_key: {:?}", _key);
					offchain.remove(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS_AT_GENESIS);

					offchain.remove(STORAGE_PREFIX, SUBMIT_GENESIS_KEYS_AT);
				}
			}

			if let Ok(Some(_sig)) = public_key_sig {
				self.refresh_in_progress = false;
				if offchain.get(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG).is_some() {
					debug!(target: "dkg", "cleaned offchain storage, next_pub_key_sig: {:?}", _sig);
					offchain.remove(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG);
				}
			}
		}
	}

	pub fn authenticate_msg_origin(
		&self,
		is_main_round: bool,
		authority_accounts: (Vec<AccountId32>, Vec<AccountId32>),
		msg: &[u8],
		signature: &[u8],
	) -> Result<sr25519::Public, DKGError> {
		let get_keys = |accts: &Vec<AccountId32>| {
			accts
				.iter()
				.map(|x| {
					sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
						panic!("Failed to convert account id to sr25519 public key")
					}))
				})
				.collect::<Vec<sr25519::Public>>()
		};

		let maybe_signers = if is_main_round {
			get_keys(&authority_accounts.0)
		} else {
			get_keys(&authority_accounts.1)
		};

		let (maybe_signer, success) =
			dkg_runtime_primitives::utils::verify_signer_from_set(maybe_signers, msg, signature);

		if !success {
			return Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority".to_string(),
			})?
		}

		Ok(maybe_signer.unwrap())
	}

	/// Generate a random delay to wait before taking an action.
	/// The delay is generated from a random number between 0 and `max_delay`.
	pub fn generate_delayed_submit_at(
		&self,
		start: NumberFor<B>,
		max_delay: u32,
	) -> Option<<B::Header as Header>::Number> {
		let mut rng = rand::thread_rng();
		let submit_at = start + rng.gen_range(0u32..=max_delay).into();
		Some(submit_at)
	}

	/// Rounds handling

	fn process_finished_rounds(&mut self) {
		if self.rounds.is_none() {
			return
		}

		let mut proposals = Vec::new();
		for finished_round in self.rounds.as_mut().unwrap().get_finished_rounds() {
			let proposal = self.handle_finished_round(finished_round);
			if proposal.is_some() {
				proposals.push(proposal.unwrap())
			}
		}
		metric_set!(
			self,
			dkg_round_concluded,
			self.rounds.as_mut().unwrap().get_finished_rounds().len()
		);

		self.process_signed_proposals(proposals);
	}

	fn handle_finished_round(&mut self, finished_round: DKGSignedPayload) -> Option<Proposal> {
		trace!(target: "dkg", "Got finished round {:?}", finished_round);
		let decoded_key =
			<(ChainIdType<ChainId>, DKGPayloadKey)>::decode(&mut &finished_round.key[..]);
		let payload_key = match decoded_key {
			Ok((_chain_id, key)) => key,
			Err(_err) => return None,
		};

		get_signed_proposal(self, finished_round.clone(), payload_key)
	}

	/// Get unsigned proposals and create offline stage using an encoded (ChainIdType<ChainId>,
	/// DKGPayloadKey) as the round key
	fn create_offline_stages(&mut self, header: &B::Header) {
		if self.rounds.is_none() {
			return
		}

		let at: BlockId<B> = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		let rounds = self.rounds.as_mut().unwrap();
		let mut errors = Vec::new();
		if rounds.is_keygen_finished() {
			for (key, ..) in &unsigned_proposals {
				if self.dkg_state.created_offlinestage_at.contains_key(&key.encode()) {
					continue
				}

				if let Err(e) = rounds.create_offline_stage(key.encode(), *header.number()) {
					error!(target: "dkg", "Failed to create offline stage: {:?}", e);
					errors.push(e);
				} else {
					// We note unsigned proposals for which we have started the offline stage
					// to prevent overwriting running offline stages when next this function is
					// called this function is called on every block import and the proposal
					// might still be in the the unsigned proposals queue.
					self.dkg_state.created_offlinestage_at.insert(key.encode(), *header.number());
				}
			}
		}

		for e in errors {
			self.handle_dkg_error(e);
		}
	}

	// *** Proposals handling ***
	/// When we create the offline stage we note that round key since the offline stage and voting
	/// process could go on for a number of blocks while the proposal is still in the unsigned
	/// proposal queue. The untrack interval is the number of blocks after which we expect the a
	/// voting round to have reached completion for a proposal After this time elapses for a round
	/// key we remove it from [dkg_state.created_offlinestage_at] since we expect that proposal to
	/// have been signed and moved to the signed proposals queue already.
	fn untrack_unsigned_proposals(&mut self, header: &B::Header) {
		let keys = self.dkg_state.created_offlinestage_at.keys().cloned().collect::<Vec<_>>();
		let _at: BlockId<B> = BlockId::hash(header.hash());
		let current_block_number = *header.number();
		metric_set!(self, dkg_votes_sent, &keys.len());
		for key in keys {
			let voted_at = self.dkg_state.created_offlinestage_at.get(&key).unwrap();
			let diff = current_block_number - *voted_at;
			let untrack_interval = <<B as Block>::Header as Header>::Number::from(
				dkg_runtime_primitives::UNTRACK_INTERVAL,
			);

			if diff >= untrack_interval {
				self.dkg_state.created_offlinestage_at.remove(&key);
			}
		}
	}

	fn process_unsigned_proposals(&mut self, header: &B::Header) {
		if self.rounds.is_none() {
			return
		}

		let latest_block_num = self.get_latest_block_number();
		let at: BlockId<B> = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());
		let rounds = self.rounds.as_mut().unwrap();
		metric_set!(self, dkg_should_vote_on, &unsigned_proposals.len());
		let mut errors = Vec::new();
		for (key, proposal) in unsigned_proposals {
			if !rounds.is_ready_to_vote(key.encode()) {
				continue
			}

			let (_chain_id_type, ..): (ChainIdType<ChainId>, DKGPayloadKey) = key.clone();
			debug!(target: "dkg", "Got unsigned proposal with key = {:?}", &key);

			if let Proposal::Unsigned { kind: _, data } = proposal {
				debug!(target: "dkg", "Got unsigned proposal with data = {:?} with key = {:?}", &data, key);
				if let Err(e) = rounds.vote(key.encode(), data, latest_block_num) {
					error!(target: "dkg", "🕸️  error creating new vote: {}", e.to_string());
					errors.push(e);
				};
			}
		}
		// Handle all errors
		for e in errors {
			self.handle_dkg_error(e);
		}
		// Send messages to all peers
		send_outgoing_dkg_messages(self);
	}

	fn process_signed_proposals(&mut self, signed_proposals: Vec<Proposal>) {
		if signed_proposals.is_empty() {
			return
		}

		debug!(target: "dkg", "🕸️  saving signed proposal in offchain storage");

		let public = fetch_public_key(self);

		if find_index::<AuthorityId>(&self.current_validator_set.authorities[..], &public).is_none()
		{
			return
		}

		// If the header is none, it means no block has been imported yet, so we can exit
		if self.latest_header.is_none() {
			return
		}

		let current_block_number = {
			let header = self.latest_header.as_ref().unwrap();
			header.number().clone()
		};

		if let Some(mut offchain) = self.backend.offchain_storage() {
			let old_val = offchain.get(STORAGE_PREFIX, OFFCHAIN_SIGNED_PROPOSALS);

			let mut prop_wrapper = match old_val.clone() {
				Some(ser_props) =>
					OffchainSignedProposals::<NumberFor<B>>::decode(&mut &ser_props[..]).unwrap(),
				None => Default::default(),
			};

			// The signed proposals are submitted in batches, since we want to try and limit
			// duplicate submissions as much as we can, we add a random submission delay to each
			// batch stored in offchain storage
			let submit_at =
				self.generate_delayed_submit_at(current_block_number, MAX_SUBMISSION_DELAY);

			if let Some(submit_at) = submit_at {
				prop_wrapper.proposals.push((signed_proposals, submit_at))
			};

			for _i in 1..STORAGE_SET_RETRY_NUM {
				if offchain.compare_and_set(
					STORAGE_PREFIX,
					OFFCHAIN_SIGNED_PROPOSALS,
					old_val.as_deref(),
					&prop_wrapper.encode(),
				) {
					debug!(target: "dkg", "🕸️  Successfully saved signed proposals in offchain storage");
					break
				}
			}
		}
	}

	// *** Main run loop ***

	pub(crate) async fn run(mut self) {
		let mut dkg =
			Box::pin(self.gossip_engine.lock().messages_for(dkg_topic::<B>()).filter_map(
				|notification| async move {
					SignedDKGMessage::<Public>::decode(&mut &notification.message[..]).ok()
				},
			));

		loop {
			let engine = self.gossip_engine.clone();
			let gossip_engine = future::poll_fn(|cx| engine.lock().poll_unpin(cx));

			futures::select! {
				notification = self.finality_notifications.next().fuse() => {
					if let Some(notification) = notification {
						self.handle_finality_notification(notification);
					} else {
						return;
					}
				},
				notification = self.block_import_notification.next().fuse() => {
					if let Some(notification) = notification {
						self.handle_import_notifications(notification);
					} else {
						return;
					}
				},
				dkg_msg = dkg.next().fuse() => {
					debug!(target: "dkg", "🕸️  Current authorities {:?}", self.current_validator_set.authorities);
					debug!(target: "dkg", "🕸️  Next authorities {:?}", self.queued_validator_set.authorities);
					if let Some(dkg_msg) = dkg_msg {
						if self.current_validator_set.authorities.is_empty() || self.queued_validator_set.authorities.is_empty() {
							self.msg_cache.push(dkg_msg);
						} else {
							let msgs = self.msg_cache.clone();
							for msg in msgs {
								match self.verify_signature_against_authorities(msg) {
									Ok(raw) => {
										debug!(target: "dkg", "🕸️  Got a cached message from gossip engine: {:?}", raw);
										self.process_incoming_dkg_message(raw);
									},
									Err(e) => {
										debug!(target: "dkg", "🕸️  Received signature error {:?}", e);
									}
								}
							}

							match self.verify_signature_against_authorities(dkg_msg) {
								Ok(raw) => {
									debug!(target: "dkg", "🕸️  Got message from gossip engine: {:?}", raw);
									self.process_incoming_dkg_message(raw);
								},
								Err(e) => {
									debug!(target: "dkg", "🕸️  Received signature error {:?}", e);
								}
							}
							// Reset the cache
							self.msg_cache = Vec::new();
						}
					} else {
						return;
					}
				},
				_ = gossip_engine.fuse() => {
					error!(target: "dkg", "🕸️  Gossip engine has terminated.");
					return;
				}
			}
		}
	}
}
