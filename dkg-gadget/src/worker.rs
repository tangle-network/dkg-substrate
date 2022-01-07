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
use dkg_primitives::{
	types::{DKGError, DKGMsgPayload, DKGPublicKeyMessage, RoundId},
	AggregatedPublicKeys, ChainId, DKGReport, ProposalType,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::{sr25519, to_slice_32},
	OffchainSignedProposals, RefreshProposal, RefreshProposalSigned, AGGREGATED_PUBLIC_KEYS,
	AGGREGATED_PUBLIC_KEYS_AT_GENESIS, GENESIS_AUTHORITY_SET_ID, OFFCHAIN_PUBLIC_KEY_SIG,
	OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_GENESIS_KEYS_AT, SUBMIT_KEYS_AT,
};

use crate::{
	error::{self},
	gossip::GossipValidator,
	metric_inc, metric_set,
	metrics::Metrics,
	types::dkg_topic,
	utils::{find_authorities_change, find_index, set_up_rounds, validate_threshold},
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

pub(crate) struct WorkerParams<B, BE, C>
where
	B: Block,
{
	pub client: Arc<C>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: GossipEngine<B>,
	pub gossip_validator: Arc<GossipValidator<B>>,
	pub min_block_delta: u32,
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
	client: Arc<C>,
	backend: Arc<BE>,
	key_store: DKGKeystore,
	gossip_engine: Arc<Mutex<GossipEngine<B>>>,
	gossip_validator: Arc<GossipValidator<B>>,
	/// Min delta in block numbers between two blocks, DKG should vote on
	min_block_delta: u32,
	metrics: Option<Metrics>,
	rounds: Option<MultiPartyECDSARounds<NumberFor<B>>>,
	next_rounds: Option<MultiPartyECDSARounds<NumberFor<B>>>,
	finality_notifications: FinalityNotifications<B>,
	block_import_notification: ImportNotifications<B>,
	/// Best block we received a GRANDPA notification for
	best_grandpa_block: NumberFor<B>,
	/// Best block a DKG voting round has been concluded for
	best_dkg_block: Option<NumberFor<B>>,
	/// Latest block header
	latest_header: Option<B::Header>,
	/// Current validator set
	current_validator_set: AuthoritySet<Public>,
	/// Queued validator set
	queued_validator_set: AuthoritySet<Public>,
	/// Validator set id for the last signed commitment
	last_signed_id: u64,
	// keep rustc happy
	_backend: PhantomData<BE>,
	// public key refresh in progress
	refresh_in_progress: bool,
	// keep track of the broadcast public keys and signatures
	aggregated_public_keys: HashMap<RoundId, AggregatedPublicKeys>,
	// dkg state
	pub dkg_state: DKGState<NumberFor<B>>,
	// Setting up keygen for current authorities
	pub active_keygen_in_progress: bool,
	// setting up queued authorities keygen
	pub queued_keygen_in_progress: bool,
	// Track DKG Persistence state
	pub dkg_persistence: DKGPersistenceState,

	pub base_path: Option<PathBuf>,
	// Concrete type that points to the actual local keystore if it exists
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
			gossip_validator,
			min_block_delta,
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
			gossip_validator,
			min_block_delta,
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
			aggregated_public_keys: HashMap::new(),
			dkg_persistence: DKGPersistenceState::new(),
			base_path,
			local_keystore,
			_backend: PhantomData,
		}
	}

	pub fn get_current_validators(&self) -> AuthoritySet<AuthorityId> {
		self.current_validator_set.clone()
	}

	pub fn get_queued_validators(&self) -> AuthoritySet<AuthorityId> {
		self.queued_validator_set.clone()
	}

	pub fn keystore_ref(&self) -> DKGKeystore {
		self.key_store.clone()
	}

	pub fn gossip_engine_ref(&self) -> Arc<Mutex<GossipEngine<B>>> {
		self.gossip_engine.clone()
	}

	pub fn set_rounds(&mut self, rounds: MultiPartyECDSARounds<NumberFor<B>>) {
		self.rounds = Some(rounds);
	}

	pub fn set_next_rounds(&mut self, rounds: MultiPartyECDSARounds<NumberFor<B>>) {
		self.next_rounds = Some(rounds);
	}

	pub fn take_rounds(&mut self) -> Option<MultiPartyECDSARounds<NumberFor<B>>> {
		self.rounds.take()
	}

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
	fn _get_authority_index(&self, header: &B::Header) -> Option<usize> {
		let new = if let Some((new, ..)) = find_authorities_change::<B>(header) {
			Some(new)
		} else {
			let at = BlockId::hash(header.hash());
			self.client.runtime_api().authority_set(&at).ok()
		};

		trace!(target: "dkg", "🕸️  active validator set: {:?}", new);

		let set = new.unwrap_or_else(|| panic!("Help"));
		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));
		for i in 0..set.authorities.len() {
			if set.authorities[i] == public {
				return Some(i)
			}
		}

		return None
	}

	pub fn get_threshold(&self, header: &B::Header) -> Option<u16> {
		let at = BlockId::hash(header.hash());
		return self.client.runtime_api().signature_threshold(&at).ok()
	}

	pub fn get_time_to_restart(&self, header: &B::Header) -> Option<NumberFor<B>> {
		let at = BlockId::hash(header.hash());
		return self.client.runtime_api().time_to_restart(&at).ok()
	}

	fn get_latest_block_number(&self) -> NumberFor<B> {
		self.latest_header.clone().unwrap().number().clone()
	}

	/// Return the next and queued validator set at header `header`.
	///
	/// Note that the validator set could be `None`. This is the case if we don't find
	/// a DKG authority set change and we can't fetch the authority set from the
	/// DKG on-chain state.
	///
	/// Such a failure is usually an indication that the DKG pallet has not been deployed (yet).
	pub fn validator_set(
		&self,
		header: &B::Header,
	) -> Option<(AuthoritySet<Public>, AuthoritySet<Public>)> {
		let new = if let Some((new, queued)) = find_authorities_change::<B>(header) {
			Some((new, queued))
		} else {
			let at = BlockId::hash(header.hash());
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
			debug!(target: "dkg", "🕸️  for block {:?} public key missing in validator set: {:?}", block, missing);
		}

		Ok(())
	}

	fn handle_dkg_setup(&mut self, header: &B::Header, next_authorities: AuthoritySet<Public>) {
		if next_authorities.authorities.is_empty() {
			return
		}

		if self.rounds.is_some() {
			if self.rounds.as_ref().unwrap().get_id() == next_authorities.id {
				return
			}
		}

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));
		let sr25519_public = self
			.key_store
			.sr25519_authority_id(&self.key_store.sr25519_public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

		let thresh = validate_threshold(
			next_authorities.authorities.len() as u16,
			self.get_threshold(header).unwrap(),
		);

		let mut local_key_path = None;

		if self.base_path.is_some() {
			local_key_path = Some(self.base_path.as_ref().unwrap().join(DKG_LOCAL_KEY_FILE));
			let _ = cleanup(local_key_path.as_ref().unwrap().clone());
		}
		let latest_block_num = self.get_latest_block_number();

		self.rounds = if self.next_rounds.is_some() {
			self.next_rounds.take()
		} else {
			if next_authorities.id == GENESIS_AUTHORITY_SET_ID &&
				find_index(&next_authorities.authorities[..], &public).is_some()
			{
				Some(set_up_rounds(
					&next_authorities,
					&public,
					&sr25519_public,
					thresh,
					local_key_path,
					*header.number(),
					self.local_keystore.clone(),
				))
			} else {
				None
			}
		};

		if next_authorities.id == GENESIS_AUTHORITY_SET_ID {
			self.dkg_state.listening_for_active_pub_key = true;

			match self
				.rounds
				.as_mut()
				.unwrap()
				.start_keygen(next_authorities.id.clone(), latest_block_num)
			{
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

	fn handle_queued_dkg_setup(&mut self, header: &B::Header, queued: AuthoritySet<Public>) {
		if queued.authorities.is_empty() {
			return
		}

		if self.next_rounds.is_some() {
			if self.next_rounds.as_ref().unwrap().get_id() == queued.id {
				return
			}
		}

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));
		let sr25519_public = self
			.key_store
			.sr25519_authority_id(&self.key_store.sr25519_public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

		let thresh = validate_threshold(
			queued.authorities.len() as u16,
			self.get_threshold(header).unwrap_or_default(),
		);

		let mut local_key_path = None;

		if self.base_path.is_some() {
			local_key_path = Some(self.base_path.as_ref().unwrap().join(QUEUED_DKG_LOCAL_KEY_FILE));
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
			));
			self.dkg_state.listening_for_pub_key = true;
			match self
				.next_rounds
				.as_mut()
				.unwrap()
				.start_keygen(queued.id.clone(), latest_block_num)
			{
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

		if let Some((active, queued)) = self.validator_set(header) {
			// Authority set change or genesis set id triggers new voting rounds
			//
			// TODO: (adoerr) Enacting a new authority set will also implicitly 'conclude'
			// the currently active DKG voting round by starting a new one. This is
			// temporary and needs to be replaced by proper round life cycle handling.
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

				// Reset refresh status
				self.refresh_in_progress = false;

				debug!(target: "dkg", "🕸️  New Rounds for id: {:?}", active.id);

				self.best_dkg_block = Some(*header.number());

				// this metric is kind of 'fake'. Best DKG block should only be updated once we have a
				// signed commitment for the block. Remove once the above TODO is done.
				metric_set!(self, dkg_best_block, *header.number());

				// Setting up the DKG
				self.handle_dkg_setup(&header, active.clone());
				self.handle_queued_dkg_setup(&header, queued.clone());

				if !self.current_validator_set.authorities.is_empty() {
					self.send_outgoing_dkg_messages();
				}
				self.dkg_state.is_epoch_over = !self.dkg_state.is_epoch_over;
			} else {
				// if the DKG has not been prepared / terminated, continue preparing it
				if !self.dkg_state.accepted || self.queued_keygen_in_progress {
					self.send_outgoing_dkg_messages();
				}
			}

			if self.queued_validator_set.authorities != queued.authorities &&
				!self.queued_keygen_in_progress
			{
				debug!(target: "dkg", "🕸️  Queued authorities changed, running key gen");
				self.queued_validator_set = queued.clone();
				self.handle_queued_dkg_setup(&header, queued.clone());
				self.send_outgoing_dkg_messages();
			}
		}

		try_restart_dkg(self, header);
		self.send_outgoing_dkg_messages();
		self.check_refresh(header);
		self.create_offline_stages(header);
		self.process_unsigned_proposals(header);
		self.untrack_unsigned_proposals(header);
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

	// *** DKG rounds ***

	fn send_outgoing_dkg_messages(&mut self) {
		debug!(target: "dkg", "🕸️  Try sending DKG messages");

		let send_messages = |rounds: &mut MultiPartyECDSARounds<NumberFor<B>>,
		                     authority_id: Public,
		                     at: NumberFor<B>|
		 -> Vec<Result<(), DKGError>> {
			let results = rounds.proceed(at);

			for message in rounds.get_outgoing_messages() {
				let dkg_message = DKGMessage {
					id: authority_id.clone(),
					payload: message,
					round_id: rounds.get_id(),
				};
				let encoded_dkg_message = dkg_message.encode();
				debug!(
					target: "dkg",
					"🕸️  DKG Message: {:?}, encoded: {:?}",
					dkg_message,
					encoded_dkg_message
				);

				let sr25519_public = self
					.key_store
					.sr25519_authority_id(&self.key_store.sr25519_public_keys().unwrap_or_default())
					.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

				match self.key_store.sr25519_sign(&sr25519_public, &encoded_dkg_message) {
					Ok(sig) => {
						let signed_dkg_message =
							SignedDKGMessage { msg: dkg_message, signature: Some(sig.encode()) };
						let encoded_signed_dkg_message = signed_dkg_message.encode();

						self.gossip_engine.lock().gossip_message(
							dkg_topic::<B>(),
							encoded_signed_dkg_message.clone(),
							true,
						);
					},
					Err(e) => trace!(
						target: "dkg",
						"🕸️  Error signing DKG message: {:?}",
						e
					),
				}
				trace!(target: "dkg", "🕸️  Sent DKG Message {:?}", encoded_dkg_message);
			}
			results
		};

		let mut keys_to_gossip = Vec::new();
		let mut rounds_send_result = vec![];
		let mut next_rounds_send_result = vec![];

		if let Some(mut rounds) = self.rounds.take() {
			if let Some(id) =
				self.key_store.authority_id(self.current_validator_set.authorities.as_slice())
			{
				debug!(target: "dkg", "🕸️  Local authority id: {:?}", id.clone());
				rounds_send_result = send_messages(&mut rounds, id, self.get_latest_block_number());
			} else {
				error!(
					"No local accounts available. Consider adding one via `author_insertKey` RPC."
				);
			}

			if self.active_keygen_in_progress {
				let is_offline_ready = rounds.is_offline_ready();
				if is_offline_ready {
					debug!(target: "dkg", "🕸️  Genesis DKGs keygen has completed");
					self.active_keygen_in_progress = false;
					let pub_key = rounds.get_public_key().unwrap().to_bytes(true).to_vec();
					let round_id = rounds.get_id();
					keys_to_gossip.push((round_id, pub_key));
				}
			}

			self.rounds = Some(rounds);
		}

		// Check if a there's a key gen process running for the queued authority set
		if self.queued_keygen_in_progress {
			if let Some(id) =
				self.key_store.authority_id(self.queued_validator_set.authorities.as_slice())
			{
				debug!(target: "dkg", "🕸️  Local authority id: {:?}", id.clone());
				if let Some(mut next_rounds) = self.next_rounds.take() {
					next_rounds_send_result =
						send_messages(&mut next_rounds, id, self.get_latest_block_number());

					let is_offline_ready = next_rounds.is_offline_ready();
					if is_offline_ready {
						debug!(target: "dkg", "🕸️  Queued DKGs keygen has completed");
						self.queued_keygen_in_progress = false;
						let pub_key = next_rounds.get_public_key().unwrap().to_bytes(true).to_vec();
						keys_to_gossip.push((next_rounds.get_id(), pub_key));
					}
					self.next_rounds = Some(next_rounds);
				}
			} else {
				error!(
					"No local accounts available. Consider adding one via `author_insertKey` RPC."
				);
			}
		}

		for (round_id, pub_key) in &keys_to_gossip {
			self.gossip_public_key(pub_key.clone(), *round_id);
		}

		for res in &rounds_send_result {
			if let Err(err) = res {
				self.handle_dkg_error(err.clone());
			}
		}

		for res in &next_rounds_send_result {
			if let Err(err) = res {
				self.handle_dkg_error(err.clone());
			}
		}
	}

	fn verify_signature_against_authorities(
		&mut self,
		signed_dkg_msg: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, String> {
		let dkg_msg = signed_dkg_msg.msg;
		let encoded = dkg_msg.encode();
		let signature = signed_dkg_msg.signature.unwrap_or_default();
		// Get authority accounts
		let mut authority_accounts: Option<(Vec<AccountId32>, Vec<AccountId32>)> = None;

		if let Some(header) = self.latest_header.as_ref() {
			let at = BlockId::hash(header.hash());
			let accounts = self.client.runtime_api().get_authority_accounts(&at).ok();

			if accounts.is_some() {
				authority_accounts = accounts;
			}
		}

		if authority_accounts.is_none() {
			return Err("No authorities".into())
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
			return Err(
				"Message signature is not from a registered authority or next authority".into()
			)
		}
	}

	fn process_incoming_dkg_message(&mut self, dkg_msg: DKGMessage<Public>) {
		debug!(target: "dkg", "🕸️  Process DKG message {}", &dkg_msg);

		if let Some(rounds) = self.rounds.as_mut() {
			if dkg_msg.round_id == rounds.get_id() {
				let block_number = {
					if self.latest_header.is_some() {
						Some(*self.latest_header.as_ref().unwrap().number())
					} else {
						None
					}
				};
				match rounds.handle_incoming(dkg_msg.payload.clone(), block_number) {
					Ok(()) => (),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}

				if rounds.is_offline_ready() {
					debug!(target: "dkg", "🕸️  DKG is ready to sign");
					self.dkg_state.accepted = true;
				}
			}
		}

		if let Some(next_rounds) = self.next_rounds.as_mut() {
			let block_number = {
				if self.latest_header.is_some() {
					Some(*self.latest_header.as_ref().unwrap().number())
				} else {
					None
				}
			};
			if next_rounds.get_id() == dkg_msg.round_id {
				debug!(target: "dkg", "🕸️  Received message for Queued DKGs");
				match next_rounds.handle_incoming(dkg_msg.payload.clone(), block_number) {
					Ok(()) => debug!(target: "dkg", "🕸️  Handled incoming messages"),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}
			}
		}

		self.handle_public_key_broadcast(dkg_msg.clone());
		self.send_outgoing_dkg_messages();
		self.process_finished_rounds();
	}

	fn handle_dkg_error(&mut self, dkg_error: DKGError) {
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
					self.handle_dkg_report(DKGReport::KeygenMisbehavior { offender }),
				DKGError::KeygenTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehavior { offender }),
				DKGError::OfflineMisbehaviour { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehavior { offender }),
				DKGError::OfflineTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehavior { offender }),
				DKGError::SignMisbehaviour { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehavior { offender }),
				DKGError::SignTimeout { bad_actors: _ } =>
					self.handle_dkg_report(DKGReport::SigningMisbehavior { offender }),
				_ => (),
			}
		}
	}

	fn handle_dkg_report(&mut self, dkg_report: DKGReport) {
		// TODO: handle report by taking slashing action
	}

	/// Offchain features

	fn listen_and_clear_offchain_storage(&mut self, header: &B::Header) {
		let at = BlockId::hash(header.hash());
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

	fn gossip_public_key(&mut self, public_key: Vec<u8>, round_id: RoundId) {
		let sr25519_public = self
			.key_store
			.sr25519_authority_id(&self.key_store.sr25519_public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"));

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find an ecdsa key in keystore"));

		if let Ok(signature) = self.key_store.sr25519_sign(&sr25519_public, &public_key) {
			let encoded_signature = signature.encode();
			let message = DKGMessage::<AuthorityId> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::PublicKeyBroadcast(DKGPublicKeyMessage {
					round_id,
					pub_key: public_key.clone(),
					signature: encoded_signature.clone(),
				}),
			};
			let encoded_dkg_message = message.encode();

			match self.key_store.sr25519_sign(&sr25519_public, &encoded_dkg_message) {
				Ok(sig) => {
					let signed_dkg_message =
						SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
					let encoded_signed_dkg_message = signed_dkg_message.encode();

					self.gossip_engine.lock().gossip_message(
						dkg_topic::<B>(),
						encoded_signed_dkg_message.clone(),
						true,
					);
				},
				Err(e) => trace!(
					target: "dkg",
					"🕸️  Error signing DKG message: {:?}",
					e
				),
			}

			let mut aggregated_public_keys = if self.aggregated_public_keys.get(&round_id).is_some()
			{
				self.aggregated_public_keys.get(&round_id).unwrap().clone()
			} else {
				AggregatedPublicKeys::default()
			};

			aggregated_public_keys
				.keys_and_signatures
				.push((public_key.clone(), encoded_signature));

			self.aggregated_public_keys.insert(round_id, aggregated_public_keys);

			debug!(target: "dkg", "gossiping local node  {:?} public key and signature", public)
		} else {
			error!(target: "dkg", "Could not sign public key");
		}
	}

	fn handle_public_key_broadcast(&mut self, dkg_msg: DKGMessage<Public>) {
		if !self.dkg_state.listening_for_pub_key && !self.dkg_state.listening_for_active_pub_key {
			return
		}

		// Get authority accounts
		let mut authority_accounts = None;
		let mut max_extrinsic_delay = None;
		if let Some(header) = self.latest_header.as_ref() {
			let at = BlockId::hash(header.hash());
			let block_number = *header.number();
			let accounts = self.client.runtime_api().get_authority_accounts(&at).ok();
			max_extrinsic_delay =
				self.client.runtime_api().get_max_extrinsic_delay(&at, block_number).ok();
			if accounts.is_some() {
				authority_accounts = accounts;
			}
		}

		if authority_accounts.is_none() {
			return
		}

		if let DKGMsgPayload::PublicKeyBroadcast(msg) = dkg_msg.payload {
			debug!(target: "dkg", "Received public key broadcast");

			let is_main_round = {
				if self.rounds.is_some() {
					msg.round_id == self.rounds.as_ref().unwrap().get_id()
				} else {
					false
				}
			};

			let can_proceed = {
				if is_main_round {
					let maybe_signers = authority_accounts
						.unwrap()
						.0
						.iter()
						.map(|x| {
							sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
								panic!("Failed to convert account id to sr25519 public key")
							}))
						})
						.collect::<Vec<sr25519::Public>>();

					dkg_runtime_primitives::utils::verify_signer_from_set(
						maybe_signers,
						&msg.pub_key,
						&msg.signature,
					)
					.1
				} else {
					let maybe_signers = authority_accounts
						.unwrap()
						.1
						.iter()
						.map(|x| {
							sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
								panic!("Failed to convert account id to sr25519 public key")
							}))
						})
						.collect::<Vec<sr25519::Public>>();

					dkg_runtime_primitives::utils::verify_signer_from_set(
						maybe_signers,
						&msg.pub_key,
						&msg.signature,
					)
					.1
				}
			};

			if !can_proceed {
				error!("Message signature is not from a registered authority");
				return
			}

			let key_and_sig = (msg.pub_key, msg.signature);
			let round_id = msg.round_id;
			let mut aggregated_public_keys = if self.aggregated_public_keys.get(&round_id).is_some()
			{
				self.aggregated_public_keys.get(&round_id).unwrap().clone()
			} else {
				AggregatedPublicKeys::default()
			};

			if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
				aggregated_public_keys.keys_and_signatures.push(key_and_sig);
				self.aggregated_public_keys.insert(round_id, aggregated_public_keys.clone());

				if let Some(latest_header) = self.latest_header.as_ref() {
					let threshold = self.get_threshold(latest_header).unwrap() as usize;
					let num_authorities = self.queued_validator_set.authorities.len();
					let threshold_buffer = num_authorities.saturating_sub(threshold) / 2;
					if aggregated_public_keys.keys_and_signatures.len() >=
						(threshold + threshold_buffer)
					{
						let offchain = self.backend.offchain_storage();

						if let Some(mut offchain) = offchain {
							if is_main_round {
								self.dkg_state.listening_for_active_pub_key = false;

								offchain.set(
									STORAGE_PREFIX,
									AGGREGATED_PUBLIC_KEYS_AT_GENESIS,
									&aggregated_public_keys.encode(),
								);

								if let Some(max_delay) = max_extrinsic_delay {
									let submit_at = self.generate_random_delay(
										&self.current_validator_set.authorities,
										max_delay,
									);
									if let Some(submit_at) = submit_at {
										offchain.set(
											STORAGE_PREFIX,
											SUBMIT_GENESIS_KEYS_AT,
											&submit_at.encode(),
										);
									}

									trace!(target: "dkg", "Stored aggregated genesis public keys {:?}, delay: {:?}, public_keysL {:?}", aggregated_public_keys.encode(), submit_at, self.key_store.sr25519_public_keys());
								}
							} else {
								self.dkg_state.listening_for_pub_key = false;

								offchain.set(
									STORAGE_PREFIX,
									AGGREGATED_PUBLIC_KEYS,
									&aggregated_public_keys.encode(),
								);

								if let Some(max_delay) = max_extrinsic_delay {
									let submit_at = self.generate_random_delay(
										&self.queued_validator_set.authorities,
										max_delay,
									);
									if let Some(submit_at) = submit_at {
										offchain.set(
											STORAGE_PREFIX,
											SUBMIT_KEYS_AT,
											&submit_at.encode(),
										);
									}

									trace!(target: "dkg", "Stored aggregated public keys {:?}, delay: {:?}, public keys: {:?}", aggregated_public_keys.encode(), submit_at, self.key_store.sr25519_public_keys());
								}
							}

							let _ = self.aggregated_public_keys.remove(&round_id);
						}
					}
				}
			}
		}
	}

	// This random delay follows an arithemtic progression
	// The min value that can be generated is the immediate next block number
	// The max value that can be generated is the block number represented
	// by max_delay
	fn generate_random_delay(
		&self,
		authorities: &Vec<AuthorityId>,
		max_delay: NumberFor<B>,
	) -> Option<<B::Header as Header>::Number> {
		if let Some(header) = self.latest_header.as_ref() {
			let public = self
				.key_store
				.authority_id(&self.key_store.public_keys().unwrap())
				.unwrap_or_else(|| panic!("Halp"));

			let party_inx = if find_index::<AuthorityId>(&authorities[..], &public).is_some() {
				find_index::<AuthorityId>(&authorities, &public).unwrap() as u32 + 1u32
			} else {
				return None
			};

			let block_number = *header.number();

			let delay = (block_number + 1u32.into()) + (max_delay % party_inx.into());
			return Some(delay)
		}
		None
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

		self.process_signed_proposals(proposals);
	}

	fn handle_finished_round(&mut self, finished_round: DKGSignedPayload) -> Option<ProposalType> {
		trace!(target: "dkg", "Got finished round {:?}", finished_round);
		let decoded_key = <(ChainId, DKGPayloadKey)>::decode(&mut &finished_round.key[..]);
		match decoded_key {
			Ok((_chain_id, DKGPayloadKey::EVMProposal(_nonce))) => Some(ProposalType::EVMSigned {
				data: finished_round.payload,
				signature: finished_round.signature,
			}),
			Ok((_chain_id, DKGPayloadKey::AnchorUpdateProposal(_nonce))) =>
				Some(ProposalType::AnchorUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				}),
			Ok((_chain_id, DKGPayloadKey::TokenAddProposal(_nonce))) =>
				Some(ProposalType::TokenAddSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				}),
			Ok((_chain_id, DKGPayloadKey::TokenRemoveProposal(_nonce))) =>
				Some(ProposalType::TokenRemoveSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				}),
			Ok((_chain_id, DKGPayloadKey::WrappingFeeUpdateProposal(_nonce))) =>
				Some(ProposalType::WrappingFeeUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				}),
			Ok((_chain_id, DKGPayloadKey::ResourceIdUpdateProposal(_nonce))) =>
				Some(ProposalType::ResourceIdUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				}),
			Ok((_chain_id, DKGPayloadKey::RefreshVote(nonce))) => {
				let offchain = self.backend.offchain_storage();

				if let Some(mut offchain) = offchain {
					let refresh_proposal = RefreshProposalSigned {
						nonce,
						signature: finished_round.signature.clone(),
					};
					let encoded_proposal = refresh_proposal.encode();
					offchain.set(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG, &encoded_proposal);

					trace!(target: "dkg", "Stored pub_key signature offchain {:?}", finished_round.signature);
				}
				None
			},
			_ => None, // TODO: handle other key types
		}
	}

	// *** Refresh Vote ***
	// Return if a refresh has been started in this worker,
	// if not check if it's rime for refresh and start signing the public key
	// for queued authorities
	fn check_refresh(&mut self, header: &B::Header) {
		if self.refresh_in_progress || self.rounds.is_none() {
			return
		}

		let latest_block_num = *header.number();
		let at = BlockId::hash(header.hash());
		let should_refresh = self.client.runtime_api().should_refresh(&at, *header.number());
		if let Ok(true) = should_refresh {
			self.refresh_in_progress = true;
			let pub_key = self.client.runtime_api().next_dkg_pub_key(&at);
			let refresh_nonce = self.client.runtime_api().refresh_nonce(&at);
			if let Ok(Some(pub_key)) = pub_key {
				match refresh_nonce {
					Ok(nonce) => {
						let key = DKGPayloadKey::RefreshVote(nonce);
						let proposal = RefreshProposal { nonce, pub_key: pub_key.clone() };

						if let Err(err) = self.rounds.as_mut().unwrap().vote(
							key.encode(),
							proposal.encode(),
							latest_block_num,
						) {
							error!(target: "dkg", "🕸️  error creating new vote: {}", err);
						} else {
							trace!(target: "dkg", "Started key refresh vote for pub_key {:?}", pub_key);
						}
						self.send_outgoing_dkg_messages();
					},
					_ => {},
				}
			}
		}
	}

	/// Get unsigned proposals and create offline stage using an encoded (ChainId, DKGPayloadKey) as the round key
	fn create_offline_stages(&mut self, header: &B::Header) {
		if self.rounds.is_none() {
			return
		}

		let at = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		let rounds = self.rounds.as_mut().unwrap();
		let mut errors = vec![];
		if rounds.is_offline_ready() {
			for (key, ..) in &unsigned_proposals {
				if self.dkg_state.created_offlinestage_at.contains_key(&key.encode()) {
					continue
				}

				match rounds.create_offline_stage(key.encode(), *header.number()) {
					Ok(()) => {
						// We note unsigned proposals for which we have started the offline stage
						// to prevent overwriting running offline stages when next this function is called
						// this function is called on every block import and the proposal might still be in the the unsigned proposals queue.
						self.dkg_state
							.created_offlinestage_at
							.insert(key.encode(), *header.number());
					},
					Err(err) => {
						error!("Error Creating offline stage {:?}", &err);
						errors.push(err)
					},
				}
			}
		}

		for err in &errors {
			self.handle_dkg_error(err.clone());
		}
	}

	// *** Proposals handling ***
	/// When we create the offline stage we note that round key since the offline stage and voting process could go on for a number of blocks while the proposal
	/// is still in the unsigned proposal queue.
	/// The untrack interval is the number of blocks after which we expect the a voting round to have reached completion for a proposal
	/// After this time elapses for a round key we remove it from [dkg_state.created_offlinestage_at] since we expect that proposal to
	/// have been signed and moved to the signed proposals queue already.
	fn untrack_unsigned_proposals(&mut self, header: &B::Header) {
		let keys = self.dkg_state.created_offlinestage_at.keys().cloned().collect::<Vec<_>>();
		let at = BlockId::hash(header.hash());
		let current_block_number = *header.number();
		for key in keys {
			let voted_at = self.dkg_state.created_offlinestage_at.get(&key).unwrap();
			let diff = current_block_number - *voted_at;
			let untrack_interval = self.client.runtime_api().untrack_interval(&at).unwrap();

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
		let at = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());
		let rounds = self.rounds.as_mut().unwrap();
		for (key, proposal) in unsigned_proposals {
			if !rounds.is_ready_to_vote(key.encode()) {
				continue
			}
			debug!(target: "dkg", "Got unsigned proposal with key = {:?}", &key);
			let data = match proposal {
				ProposalType::EVMUnsigned { data } => data,
				ProposalType::AnchorUpdate { data } => data,
				ProposalType::TokenAdd { data } => data,
				ProposalType::TokenRemove { data } => data,
				ProposalType::WrappingFeeUpdate { data } => data,
				_ => continue,
			};

			if let Err(err) = rounds.vote(key.encode(), data, latest_block_num) {
				error!(target: "dkg", "🕸️  error creating new vote: {}", err);
			}
		}
		// send messages to all peers
		self.send_outgoing_dkg_messages();
	}

	fn process_signed_proposals(&mut self, signed_proposals: Vec<ProposalType>) {
		if signed_proposals.is_empty() {
			return
		}

		debug!(target: "dkg", "🕸️  saving signed proposal in offchain starage");

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));

		if find_index::<AuthorityId>(&self.current_validator_set.authorities[..], &public).is_none()
		{
			return
		}

		// If the header is none, it means no block has been imported yet, so we can exit
		if self.latest_header.is_none() {
			return
		}

		let untrack_interval = {
			let header = self.latest_header.as_ref().unwrap();
			let at = BlockId::hash(header.hash());
			self.client.runtime_api().untrack_interval(&at).unwrap()
		};

		if let Some(mut offchain) = self.backend.offchain_storage() {
			let old_val = offchain.get(STORAGE_PREFIX, OFFCHAIN_SIGNED_PROPOSALS);

			let mut prop_wrapper = match old_val.clone() {
				Some(ser_props) =>
					OffchainSignedProposals::<NumberFor<B>>::decode(&mut &ser_props[..]).unwrap(),
				None => OffchainSignedProposals::<NumberFor<B>>::default(),
			};

			// The signed proposals are submitted in batches, since we want to try and limit duplicate submissions
			// as much as we can, we add a random submission delay to each batch stored in offchain storage
			// We use the untrack_interval as the max submission delay here since it's a relatively short period of time.
			let submit_at = self
				.generate_random_delay(&self.current_validator_set.authorities, untrack_interval);

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
					// debug!(target: "dkg", "🕸️  Got message: {:?}", notification);

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
					if let Some(dkg_msg) = dkg_msg {
						if let Ok(raw) = self.verify_signature_against_authorities(dkg_msg.clone()) {
							self.process_incoming_dkg_message(raw);
						} else {
							error!(target: "dkg", "🕸️  Received message with invalid signature");
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

#[cfg(test)]
mod tests {

	#[test]
	fn dummy_test() {
		assert_eq!(1, 1)
	}
}
