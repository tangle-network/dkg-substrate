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

use core::convert::TryFrom;
use curv::elliptic::curves::traits::ECPoint;
use std::{
	collections::{BTreeSet, HashMap},
	marker::PhantomData,
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
	generic::OpaqueDigestItemId,
	traits::{Block, Header, NumberFor},
};

use crate::keystore::DKGKeystore;
use dkg_primitives::{
	types::{DKGMsgPayload, DKGPublicKeyMessage, RoundId},
	AggregatedPublicKeys, ProposalType,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::{sr25519, to_slice_32},
	ConsensusLog, MmrRootHash, OffchainSignedProposals, AGGREGATED_PUBLIC_KEYS,
	AGGREGATED_PUBLIC_KEYS_AT_GENESIS, GENESIS_AUTHORITY_SET_ID, OFFCHAIN_PUBLIC_KEY_SIG,
	OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_GENESIS_KEYS_AT, SUBMIT_KEYS_AT,
};

use crate::{
	error::{self},
	gossip::GossipValidator,
	metric_inc, metric_set,
	metrics::Metrics,
	types::dkg_topic,
	Client,
};

use dkg_primitives::{
	rounds::{DKGState, MultiPartyECDSARounds},
	types::{DKGMessage, DKGPayloadKey, DKGSignedPayload},
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
	pub dkg_state: DKGState<DKGPayloadKey>,
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
	rounds: Option<MultiPartyECDSARounds<DKGPayloadKey>>,
	next_rounds: Option<MultiPartyECDSARounds<DKGPayloadKey>>,
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
	// dkg state
	dkg_state: DKGState<DKGPayloadKey>,
	// setting up queued authorities keygen
	queued_keygen_in_progress: bool,
	// Setting up keygen for genesis authorities
	genesis_keygen_in_progress: bool,
	// public key refresh in progress
	refresh_in_progress: bool,
	// keep track of the broadcast public keys and signatures
	aggregated_public_keys: HashMap<RoundId, AggregatedPublicKeys>,
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
			genesis_keygen_in_progress: false,
			refresh_in_progress: false,
			aggregated_public_keys: HashMap::new(),
			_backend: PhantomData,
		}
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

	fn get_threshold(&self, header: &B::Header) -> Option<u16> {
		let at = BlockId::hash(header.hash());
		return self.client.runtime_api().signature_threshold(&at).ok()
	}

	/// Return the next and queued validator set at header `header`.
	///
	/// Note that the validator set could be `None`. This is the case if we don't find
	/// a DKG authority set change and we can't fetch the authority set from the
	/// DKG on-chain state.
	///
	/// Such a failure is usually an indication that the DKG pallet has not been deployed (yet).
	fn validator_set(
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

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));

		let thresh = validate_threshold(
			next_authorities.authorities.len() as u16,
			self.get_threshold(header).unwrap(),
		);

		self.rounds = if self.next_rounds.is_some() {
			self.next_rounds.take()
		} else {
			if next_authorities.id == GENESIS_AUTHORITY_SET_ID &&
				find_index(&next_authorities.authorities, &public).is_some()
			{
				Some(set_up_rounds(&next_authorities, &public, thresh))
			} else {
				None
			}
		};

		if next_authorities.id == GENESIS_AUTHORITY_SET_ID {
			self.dkg_state.listening_for_genesis_pub_key = true;

			match self.rounds.as_mut().unwrap().start_keygen(next_authorities.id.clone()) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for genesis authority set successfully");
					self.genesis_keygen_in_progress = true;
				},
				Err(err) => error!("Error starting keygen {}", err),
			}
		}
	}

	fn handle_queued_dkg_setup(&mut self, header: &B::Header, queued: AuthoritySet<Public>) {
		if queued.authorities.is_empty() {
			return
		}

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));

		let thresh = validate_threshold(
			queued.authorities.len() as u16,
			self.get_threshold(header).unwrap_or_default(),
		);

		// If current node is part of the queued authorities
		// start the multiparty keygen process
		if queued.authorities.contains(&public) {
			// Setting up DKG for queued authorities
			self.next_rounds = Some(set_up_rounds(&queued, &public, thresh));
			self.dkg_state.listening_for_pub_key = true;
			match self.next_rounds.as_mut().unwrap().start_keygen(queued.id.clone()) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for queued authority set successfully");
					self.queued_keygen_in_progress = true;
				},
				Err(err) => error!("Error starting keygen {}", err),
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

		self.check_refresh(header);
		self.process_unsigned_proposals(&header);
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

		let send_messages = |rounds: &mut MultiPartyECDSARounds<DKGPayloadKey>,
		                     authority_id: Public| {
			rounds.proceed();

			// TODO: run this in a different place, tied to certain number of blocks probably
			if rounds.is_offline_ready() {
				// TODO: use deterministic random signers set
				let signer_set_id = rounds.get_id();
				let s_l = (1..=rounds.dkg_params().2).collect();
				match rounds.reset_signers(signer_set_id, s_l) {
					Ok(()) => info!(target: "dkg", "🕸️  Reset signers"),
					Err(err) => error!("Error resetting signers {}", err),
				}
			}

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

				self.gossip_engine.lock().gossip_message(
					dkg_topic::<B>(),
					encoded_dkg_message.clone(),
					true,
				);
				trace!(target: "dkg", "🕸️  Sent DKG Message {:?}", encoded_dkg_message);
			}
		};

		let mut keys_to_gossip = Vec::new();

		if let Some(mut rounds) = self.rounds.take() {
			if let Some(id) =
				self.key_store.authority_id(self.current_validator_set.authorities.as_slice())
			{
				debug!(target: "dkg", "🕸️  Local authority id: {:?}", id.clone());

				send_messages(&mut rounds, id);
			} else {
				error!(
					"No local accounts available. Consider adding one via `author_insertKey` RPC."
				);
			}

			if self.genesis_keygen_in_progress &&
				self.current_validator_set.id == GENESIS_AUTHORITY_SET_ID
			{
				let is_ready_to_vote = rounds.is_ready_to_vote();
				if is_ready_to_vote {
					debug!(target: "dkg", "🕸️  Genesis DKGs keygen has completed");
					self.genesis_keygen_in_progress = false;
					let pub_key =
						rounds.get_public_key().unwrap().get_element().serialize().to_vec();
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
					send_messages(&mut next_rounds, id);

					let is_ready_to_vote = next_rounds.is_ready_to_vote();
					debug!(target: "dkg", "🕸️  Is ready to to vote {:?}", is_ready_to_vote);
					if is_ready_to_vote {
						debug!(target: "dkg", "🕸️  Queued DKGs keygen has completed");
						self.queued_keygen_in_progress = false;
						let pub_key = next_rounds
							.get_public_key()
							.unwrap()
							.get_element()
							.serialize()
							.to_vec();
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
	}

	fn process_incoming_dkg_message(&mut self, dkg_msg: DKGMessage<Public, DKGPayloadKey>) {
		debug!(target: "dkg", "🕸️  Process DKG message {}", &dkg_msg);

		if let Some(rounds) = self.rounds.as_mut() {
			if dkg_msg.round_id == rounds.get_id() {
				match rounds.handle_incoming(dkg_msg.payload.clone()) {
					Ok(()) => (),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}

				if rounds.is_ready_to_vote() {
					debug!(target: "dkg", "🕸️  DKG is ready to sign");
					self.dkg_state.accepted = true;
				}
			}
		}

		if let Some(next_rounds) = self.next_rounds.as_mut() {
			if next_rounds.get_id() == dkg_msg.round_id {
				debug!(target: "dkg", "🕸️  Received message for Queued DKGs");
				match next_rounds.handle_incoming(dkg_msg.payload.clone()) {
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
			let message = DKGMessage::<AuthorityId, DKGPayloadKey> {
				id: public.clone(),
				round_id,
				payload: DKGMsgPayload::PublicKeyBroadcast(DKGPublicKeyMessage {
					round_id,
					pub_key: public_key.clone(),
					signature: encoded_signature.clone(),
				}),
			};

			self.gossip_engine
				.lock()
				.gossip_message(dkg_topic::<B>(), message.encode(), true);

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

	fn handle_public_key_broadcast(&mut self, dkg_msg: DKGMessage<Public, DKGPayloadKey>) {
		if !self.dkg_state.listening_for_pub_key && !self.dkg_state.listening_for_genesis_pub_key {
			return
		}

		// Get authority accounts
		let mut authority_accounts = None;

		if let Some(header) = self.latest_header.as_ref() {
			let at = BlockId::hash(header.hash());
			let accounts = self.client.runtime_api().get_authority_accounts(&at).ok();

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
								self.dkg_state.listening_for_genesis_pub_key = false;

								offchain.set(
									STORAGE_PREFIX,
									AGGREGATED_PUBLIC_KEYS_AT_GENESIS,
									&aggregated_public_keys.encode(),
								);

								let submit_at = self
									.generate_random_delay(&self.current_validator_set.authorities);
								if let Some(submit_at) = submit_at {
									offchain.set(
										STORAGE_PREFIX,
										SUBMIT_GENESIS_KEYS_AT,
										&submit_at.encode(),
									);
								}

								trace!(target: "dkg", "Stored aggregated genesis public keys {:?}, delay: {:?}, public_keysL {:?}", aggregated_public_keys.encode(), submit_at, self.key_store.sr25519_public_keys());
							} else {
								self.dkg_state.listening_for_pub_key = false;

								offchain.set(
									STORAGE_PREFIX,
									AGGREGATED_PUBLIC_KEYS,
									&aggregated_public_keys.encode(),
								);
								let submit_at = self
									.generate_random_delay(&self.queued_validator_set.authorities);
								if let Some(submit_at) = submit_at {
									offchain.set(
										STORAGE_PREFIX,
										SUBMIT_KEYS_AT,
										&submit_at.encode(),
									);
								}

								trace!(target: "dkg", "Stored aggregated public keys {:?}, delay: {:?}, public keys: {:?}", aggregated_public_keys.encode(), submit_at, self.key_store.sr25519_public_keys());
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
	// by 50% of the keygen refresh delay period
	fn generate_random_delay(
		&self,
		authorities: &Vec<AuthorityId>,
	) -> Option<<B::Header as Header>::Number> {
		if let Some(header) = self.latest_header.as_ref() {
			let public = self
				.key_store
				.authority_id(&self.key_store.public_keys().unwrap())
				.unwrap_or_else(|| panic!("Halp"));

			let party_inx = find_index::<AuthorityId>(&authorities, &public).unwrap() as u32 + 1u32;

			let block_number = *header.number();
			let at = BlockId::hash(header.hash());
			let max_delay =
				self.client.runtime_api().get_max_extrinsic_delay(&at, block_number).ok();
			match max_delay {
				Some(max_delay) => {
					let delay = (block_number + 1u32.into()) + (max_delay % party_inx.into());
					return Some(delay)
				},
				None => return None,
			}
		}
		None
	}

	/// Rounds handling

	fn process_finished_rounds(&mut self) {
		if self.rounds.is_none() {
			return
		}

		for finished_round in self.rounds.as_mut().unwrap().get_finished_rounds() {
			self.handle_finished_round(finished_round);
		}
	}

	fn handle_finished_round(&mut self, finished_round: DKGSignedPayload<DKGPayloadKey>) {
		trace!(target: "dkg", "Got finished round {:?}", finished_round);
		match finished_round.key {
			DKGPayloadKey::EVMProposal(_nonce) => {
				self.process_signed_proposal(ProposalType::EVMSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				});
			},
			DKGPayloadKey::AnchorUpdateProposal(_nonce) => {
				self.process_signed_proposal(ProposalType::AnchorUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				});
			},
			DKGPayloadKey::RefreshVote(_nonce) => {
				let offchain = self.backend.offchain_storage();

				if let Some(mut offchain) = offchain {
					offchain.set(
						STORAGE_PREFIX,
						OFFCHAIN_PUBLIC_KEY_SIG,
						&finished_round.signature,
					);

					trace!(target: "dkg", "Stored  pub _key signature offchain {:?}", finished_round.signature);
				}
			},
			DKGPayloadKey::TokenUpdateProposal(_nonce) => {
				self.process_signed_proposal(ProposalType::TokenUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				});
			},
			DKGPayloadKey::WrappingFeeUpdateProposal(_nonce) => {
				self.process_signed_proposal(ProposalType::WrappingFeeUpdateSigned {
					data: finished_round.payload,
					signature: finished_round.signature,
				});
			},
			// TODO: handle other key types
		};
	}

	// *** Refresh Vote ***
	// Return if a refresh has been started in this worker,
	// if not check if it's rime for refresh and start signing the public key
	// for queued authorities
	fn check_refresh(&mut self, header: &B::Header) {
		if self.refresh_in_progress || self.rounds.is_none() {
			return
		}

		let at = BlockId::hash(header.hash());
		let should_refresh = self.client.runtime_api().should_refresh(&at, *header.number());
		if let Ok(true) = should_refresh {
			self.refresh_in_progress = true;
			let pub_key = self.client.runtime_api().next_dkg_pub_key(&at);
			if let Ok(Some(pub_key)) = pub_key {
				let key = DKGPayloadKey::RefreshVote(self.current_validator_set.id + 1u64);

				if let Err(err) = self.rounds.as_mut().unwrap().vote(key, pub_key.clone()) {
					error!(target: "dkg", "🕸️  error creating new vote: {}", err);
				} else {
					trace!(target: "dkg", "Started key refresh vote for pub_key {:?}", pub_key);
				}
				self.send_outgoing_dkg_messages();
			}
		}
	}

	// *** Proposals handling ***

	fn process_unsigned_proposals(&mut self, header: &B::Header) {
		if self.rounds.is_none() {
			return
		}

		let at = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

		for (key, proposal) in unsigned_proposals {
			debug!(target: "dkg", "Got unsigned proposal with key = {:?}", key);
			let data = match proposal {
				ProposalType::EVMUnsigned { data } => data,
				ProposalType::AnchorUpdate { data } => data,
				ProposalType::TokenUpdate { data } => data,
				ProposalType::WrappingFeeUpdate { data } => data,
				_ => continue,
			};

			if let Err(err) = self.rounds.as_mut().unwrap().vote(key, data) {
				error!(target: "dkg", "🕸️  error creating new vote: {}", err);
			}
			// send messages to all peers
			self.send_outgoing_dkg_messages();
		}
	}

	fn process_signed_proposal(&mut self, signed_proposal: ProposalType) {
		debug!(target: "dkg", "🕸️  saving signed proposal in offchain starage");

		if let Some(mut offchain) = self.backend.offchain_storage() {
			let old_val = offchain.get(STORAGE_PREFIX, OFFCHAIN_SIGNED_PROPOSALS);

			let mut prop_wrapper = match old_val.clone() {
				Some(ser_props) => OffchainSignedProposals::decode(&mut &ser_props[..]).unwrap(),
				None => OffchainSignedProposals::default(),
			};

			prop_wrapper.proposals.push_back(signed_proposal);

			for _i in 1..STORAGE_SET_RETRY_NUM {
				if offchain.compare_and_set(
					STORAGE_PREFIX,
					OFFCHAIN_SIGNED_PROPOSALS,
					old_val.as_deref(),
					&prop_wrapper.encode(),
				) {
					debug!(target: "dkg", "🕸️  Successfully saved signed proposal in offchain starage");
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

					DKGMessage::<Public, DKGPayloadKey>::decode(&mut &notification.message[..]).ok()
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
						self.process_incoming_dkg_message(dkg_msg);
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

fn find_index<B: Eq>(queue: &Vec<B>, value: &B) -> Option<usize> {
	for (i, v) in queue.iter().enumerate() {
		if value == v {
			return Some(i)
		}
	}
	None
}

fn validate_threshold(n: u16, t: u16) -> u16 {
	let max_thresh = n - 1;
	if t >= 1 && t <= max_thresh {
		return t
	}

	return max_thresh
}

fn set_up_rounds(
	authority_set: &AuthoritySet<Public>,
	public: &Public,
	thresh: u16,
) -> MultiPartyECDSARounds<DKGPayloadKey> {
	let party_inx = find_index::<AuthorityId>(&authority_set.authorities, public).unwrap() + 1;

	let n = authority_set.authorities.len();

	let rounds = MultiPartyECDSARounds::new(
		u16::try_from(party_inx).unwrap(),
		thresh,
		u16::try_from(n).unwrap(),
		authority_set.id.clone(),
	);

	rounds
}

/// Extract the MMR root hash from a digest in the given header, if it exists.
fn find_mmr_root_digest<B, Id>(header: &B::Header) -> Option<MmrRootHash>
where
	B: Block,
	Id: Codec,
{
	header.digest().logs().iter().find_map(|log| {
		match log.try_to::<ConsensusLog<Id>>(OpaqueDigestItemId::Consensus(&ENGINE_ID)) {
			Some(ConsensusLog::MmrRoot(root)) => Some(root),
			_ => None,
		}
	})
}

/// Scan the `header` digest log for a DKG validator set change. Return either the new
/// validator set or `None` in case no validator set change has been signaled.
fn find_authorities_change<B>(
	header: &B::Header,
) -> Option<(AuthoritySet<AuthorityId>, AuthoritySet<AuthorityId>)>
where
	B: Block,
{
	let id = OpaqueDigestItemId::Consensus(&ENGINE_ID);

	let filter = |log: ConsensusLog<AuthorityId>| match log {
		ConsensusLog::AuthoritiesChange {
			next_authorities: validator_set,
			next_queued_authorities,
		} => Some((validator_set, next_queued_authorities)),
		_ => None,
	};

	header.digest().convert_first(|l| l.try_to(id).and_then(filter))
}

#[cfg(test)]
mod tests {

	#[test]
	fn dummy_test() {
		assert_eq!(1, 1)
	}
}
