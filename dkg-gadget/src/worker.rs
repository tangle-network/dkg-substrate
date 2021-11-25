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
use std::{collections::BTreeSet, marker::PhantomData, sync::Arc};

use codec::{Codec, Decode, Encode};
use futures::{future, FutureExt, StreamExt};
use log::{debug, error, info, trace};
use parking_lot::Mutex;

use sc_client_api::{
	Backend, BlockImportNotification, FinalityNotification, FinalityNotifications,
	ImportNotifications,
};
use sc_network_gossip::GossipEngine;

use sp_api::BlockId;
use sp_runtime::{
	generic::OpaqueDigestItemId,
	traits::{Block, Header, NumberFor},
};

use dkg_primitives::ProposalType;

use crate::keystore::DKGKeystore;

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	ConsensusLog, MmrRootHash, GENESIS_AUTHORITY_SET_ID,
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
	rounds: MultiPartyECDSARounds<DKGPayloadKey>,
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
}

impl<B, C, BE> DKGWorker<B, C, BE>
where
	B: Block + Codec,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId>,
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
			rounds: MultiPartyECDSARounds::new(0, 0, 1, 0),
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
			_backend: PhantomData,
		}
	}
}

impl<B, C, BE> DKGWorker<B, C, BE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId>,
{
	fn get_authority_index(&self, header: &B::Header) -> Option<usize> {
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

	fn handle_dkg_processing(
		&mut self,
		header: &B::Header,
		next_authorities: AuthoritySet<Public>,
		queued: AuthoritySet<Public>,
	) {
		if next_authorities.authorities.is_empty() {
			return
		}

		let public = self
			.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"));

		let thresh = self.get_threshold(header).unwrap();

		let set_up_rounds = |authority_set: &AuthoritySet<Public>, public: &Public| {
			let party_inx = find_index::<AuthorityId>(&authority_set.authorities, public).unwrap();

			let n = authority_set.authorities.len();

			let rounds = MultiPartyECDSARounds::new(
				u16::try_from(party_inx).unwrap(),
				thresh,
				u16::try_from(n).unwrap(),
				authority_set.id.clone(),
			);

			rounds
		};

		self.rounds = self
			.next_rounds
			.take()
			.unwrap_or_else(|| set_up_rounds(&next_authorities, &public));

		if next_authorities.id == GENESIS_AUTHORITY_SET_ID {
			match self.rounds.start_keygen(next_authorities.id.clone()) {
				Ok(()) =>
					info!(target: "dkg", "Keygen started for next authority set successfully"),
				Err(err) => error!("Error starting keygen {}", err),
			}
		}

		if queued.authorities.contains(&public) {
			// Setting up DKG for queued authorities
			self.next_rounds = Some(set_up_rounds(&queued, &public));

			match self.next_rounds.as_mut().unwrap().start_keygen(queued.id.clone()) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for next authority set successfully");
					self.queued_keygen_in_progress = true;
				},
				Err(err) => error!("Error starting keygen {}", err),
			}
		}
	}

	// *** Block notifications ***

	fn process_block_notification(&mut self, header: &B::Header) {
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
				self.latest_header = Some(header.clone());

				debug!(target: "dkg", "🕸️  New Rounds for id: {:?}", active.id);

				self.best_dkg_block = Some(*header.number());

				// this metric is kind of 'fake'. Best DKG block should only be updated once we have a
				// signed commitment for the block. Remove once the above TODO is done.
				metric_set!(self, dkg_best_block, *header.number());

				// Setting up the DKG
				self.handle_dkg_processing(&header, active.clone(), queued.clone());

				if !self.current_validator_set.authorities.is_empty() {
					self.send_outgoing_dkg_messages();
				}
				self.dkg_state.is_epoch_over = !self.dkg_state.is_epoch_over;
			} else {
				// if the DKG has not been prepared / terminated, continue preparing it
				if !self.dkg_state.accepted {
					self.send_outgoing_dkg_messages();
				}
			}
		}

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

			if rounds.is_offline_ready() {
				let at = BlockId::hash(self.latest_header.as_ref().unwrap().hash());
				let pub_key = rounds
					.get_public_key()
					.unwrap()
					.get_element()
					.serialize_uncompressed()
					.to_vec();
				let _res = self.client.runtime_api().set_dkg_pub_key(&at, pub_key);
			}

			// TODO: run this in a different place, tied to certain number of blocks probably
			if rounds.is_offline_ready() {
				// TODO: use deterministic random signers set
				let signer_set_id = self.current_validator_set.id;
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

		if let Some(id) =
			self.key_store.authority_id(self.current_validator_set.authorities.as_slice())
		{
			debug!(target: "dkg", "🕸️  Local authority id: {:?}", id);
			send_messages(&mut self.rounds, id);
		} else {
			panic!("error");
		}

		if self.queued_keygen_in_progress {
			if let Some(id) =
				self.key_store.authority_id(self.queued_validator_set.authorities.as_slice())
			{
				debug!(target: "dkg", "🕸️  Local authority id: {:?}", id);
				if let Some(next_rounds) = self.next_rounds.as_mut() {
					send_messages(next_rounds, id);
				}
			} else {
				panic!("error");
			}
		}
	}

	fn process_incoming_dkg_message(&mut self, dkg_msg: DKGMessage<Public, DKGPayloadKey>) {
		debug!(target: "dkg", "🕸️  Process DKG message {}", &dkg_msg);

		if dkg_msg.round_id == self.rounds.get_id() {
			match self.rounds.handle_incoming(dkg_msg.payload.clone()) {
				Ok(()) => (),
				Err(err) => debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
			}

			if self.rounds.is_ready_to_vote() {
				debug!(target: "dkg", "🕸️  DKG is ready to sign");
				self.dkg_state.accepted = true;
			}
		}

		if let Some(mut next_rounds) = self.next_rounds.take() {
			if next_rounds.get_id() == dkg_msg.round_id {
				match next_rounds.handle_incoming(dkg_msg.payload) {
					Ok(()) => (),
					Err(err) =>
						debug!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
				}

				self.next_rounds = Some(next_rounds);

				if self.next_rounds.as_mut().unwrap().is_ready_to_vote() {
					debug!(target: "dkg", "🕸️  Queued DKGs keygen has completed");
					self.queued_keygen_in_progress = false;
				}
			}
		}

		self.send_outgoing_dkg_messages();

		self.process_finished_rounds();
	}

	fn process_finished_rounds(&mut self) {
		for finished_round in self.rounds.get_finished_rounds() {
			self.handle_finished_round(finished_round);
		}

		if let Some(mut next_rounds) = self.next_rounds.take() {
			for finished_round in next_rounds.get_finished_rounds() {
				self.handle_finished_round(finished_round);
			}
			self.next_rounds = Some(next_rounds)
		}
	}

	fn handle_finished_round(&mut self, finished_round: DKGSignedPayload<DKGPayloadKey>) {
		match finished_round.key {
			DKGPayloadKey::EVMProposal(_nonce) => {
				self.process_signed_proposal(finished_round);
			},
			// TODO: handle other key types
		};
	}

	// *** Proposals handling ***

	fn process_unsigned_proposals(&mut self, header: &B::Header) {
		let at = BlockId::hash(header.hash());
		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		for (nonce, proposal) in unsigned_proposals {
			let key = DKGPayloadKey::EVMProposal(nonce);
			let data = match proposal {
				ProposalType::EVMUnsigned { ref data } => data.clone(),
				_ => continue,
			};

			if !self.rounds.has_vote_in_process(key) {
				if let Err(err) = self.rounds.vote(key, data) {
					error!(target: "dkg", "🕸️  error creating new vote: {}", err);
				}
			}
		}
	}

	fn process_signed_proposal(&mut self, signed_payload: DKGSignedPayload<DKGPayloadKey>) {
		let signed_proposal = ProposalType::EVMSigned {
			data: signed_payload.payload,
			signature: signed_payload.signature,
		};

		// TODO: Submit signed proposal extrinsic either using offchain context or directly w/ subxt
		// let at: Block = BlockId::hash(self.latest_header.as_ref().unwrap().hash());
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
