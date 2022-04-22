// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(clippy::collapsible_match)]

use sc_keystore::LocalKeystore;
use sp_core::{ecdsa, sr25519};
use std::{
	collections::{hash_map::RandomState, BTreeSet, HashMap, HashSet},
	marker::PhantomData,
	path::PathBuf,
	sync::Arc,
};

use codec::{Codec, Decode, Encode};

use dkg_runtime_primitives::utils::to_slice_32;
use futures::{future, FutureExt, StreamExt};
use log::{debug, error, info, trace};

use parking_lot::{Mutex, RwLock};

use sc_client_api::{
	Backend, BlockImportNotification, FinalityNotification, FinalityNotifications,
	ImportNotifications,
};
use sc_network_gossip::GossipEngine;

use rand::Rng;
use sp_api::BlockId;
use sp_core::crypto::AccountId32;
use sp_runtime::traits::{Block, Header, NumberFor};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
	keystore::DKGKeystore,
	persistence::{try_resume_dkg, DKGPersistenceState},
};

use crate::messages::{
	dkg_message::send_outgoing_dkg_messages, misbehaviour_report::gossip_misbehaviour_report,
};

use crate::storage::{
	clear::listen_and_clear_offchain_storage, proposals::save_signed_proposals_in_storage,
};

use dkg_primitives::{
	types::{DKGError, DKGMisbehaviourMessage, RoundId},
	AuthoritySetId, DKGReport, MisbehaviourType, Proposal, GOSSIP_MESSAGE_RESENDING_LIMIT,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::to_slice_33,
	AggregatedMisbehaviourReports, AggregatedPublicKeys, TypedChainId, UnsignedProposal,
	GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	error, metric_set,
	metrics::Metrics,
	proposal::get_signed_proposal,
	types::dkg_topic,
	utils::{find_authorities_change, get_key_path},
	Client,
};

use crate::async_protocol_handlers::{meta_channel::DKGIface, SignedMessageBroadcastHandle};
use dkg_primitives::{
	rounds::{DKGState, MultiPartyECDSARounds},
	types::{DKGMessage, DKGPayloadKey, DKGSignedPayload, SignedDKGMessage},
	utils::{cleanup, DKG_LOCAL_KEY_FILE, QUEUED_DKG_LOCAL_KEY_FILE},
};
use dkg_runtime_primitives::{AuthoritySet, DKGApi};
use crate::async_protocol_handlers::meta_channel::{MetaAsyncProtoStatusHandle, MetaDKGMessageHandler};

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
	pub to_async_proto: tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: Arc<Mutex<GossipEngine<B>>>,
	pub metrics: Option<Metrics>,
	pub rounds: Option<MetaAsyncProtoStatusHandle>,
	pub next_rounds: Option<MetaAsyncProtoStatusHandle>,
	pub finality_notifications: FinalityNotifications<B>,
	pub import_notifications: ImportNotifications<B>,
	pub votes_sent: u64,
	/// Best block a DKG voting round has been concluded for
	pub best_dkg_block: Option<NumberFor<B>>,
	/// Latest block header
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
	/// Current validator set
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	/// Queued validator set
	pub queued_validator_set: AuthoritySet<Public>,
	/// Msg cache for startup if authorities aren't set
	pub msg_cache: Vec<SignedDKGMessage<AuthorityId>>,
	/// Tracking for the broadcasted public keys and signatures
	pub aggregated_public_keys: Arc<Mutex<HashMap<RoundId, AggregatedPublicKeys>>>,
	/// Tracking for the misbehaviour reports
	pub aggregated_misbehaviour_reports: HashMap<
		(MisbehaviourType, RoundId, AuthorityId),
		AggregatedMisbehaviourReports<AuthorityId>,
	>,
	/// Tracking for sent gossip messages: using blake2_128 for message hashes
	/// The value is the number of times the message has been sent.
	pub has_sent_gossip_msg: HashMap<[u8; 16], u8>,
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
	pub unsigned_proposals_tx: Option<UnboundedSender<Vec<UnsignedProposal>>>,
	// keep rustc happy
	_backend: PhantomData<BE>,
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

		// we drop the rx handle since we can call tx.subscribe() every time we need to fire up
		// a new AsyncProto
		let (to_async_proto, _) = tokio::sync::broadcast::channel(10);

		DKGWorker {
			client: client.clone(),
			to_async_proto,
			backend,
			key_store,
			gossip_engine: Arc::new(Mutex::new(gossip_engine)),
			metrics,
			votes_sent: 0,
			rounds: None,
			next_rounds: None,
			finality_notifications: client.finality_notification_stream(),
			import_notifications: client.import_notification_stream(),
			best_dkg_block: None,
			current_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			queued_validator_set: AuthoritySet::empty(),
			latest_header: Arc::new(RwLock::new(None)),
			dkg_state,
			queued_keygen_in_progress: false,
			active_keygen_in_progress: false,
			msg_cache: Vec::new(),
			aggregated_public_keys: Arc::new(Mutex::new(HashMap::new())),
			aggregated_misbehaviour_reports: HashMap::new(),
			has_sent_gossip_msg: HashMap::new(),
			dkg_persistence: DKGPersistenceState::new(),
			base_path,
			local_keystore,
			unsigned_proposals_tx: None,
			_backend: PhantomData,
		}
	}
}

pub struct AsyncProtocolParameters<BCIface> {
	pub blockchain_iface: Arc<BCIface>,
	pub keystore: DKGKeystore,
	pub signed_message_broadcast_handle: SignedMessageBroadcastHandle,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	pub unsigned_proposals_rx: Arc<Mutex<Option<UnboundedReceiver<Vec<UnsignedProposal>>>>>,
}

// Manual implementation of Clone due to https://stegosaurusdormant.com/understanding-derive-clone/
impl<BCIface> Clone for AsyncProtocolParameters<BCIface> {
	fn clone(&self) -> Self {
		Self {
			blockchain_iface: self.blockchain_iface.clone(),
			keystore: self.keystore.clone(),
			signed_message_broadcast_handle: self.signed_message_broadcast_handle.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities: self.best_authorities.clone(),
			authority_public_key: self.authority_public_key.clone(),
			unsigned_proposals_rx: self.unsigned_proposals_rx.clone(),
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
	// NOTE: This must be ran at the start of each epoch since best_authorities may change
	// if "current" is true, this will set the "rounds" field in the dkg worker, otherwise,
	// it well set the "next_rounds" field
	fn generate_async_proto_params(
		&mut self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		current: bool
	) -> AsyncProtocolParameters<DKGIface<B, BE, C>> {
		let best_authorities = Arc::new(best_authorities);
		let authority_public_key = Arc::new(authority_public_key);

		let (unsigned_proposals_tx, unsigned_proposals_rx) = tokio::sync::mpsc::unbounded_channel();
		let status_handle = MetaAsyncProtoStatusHandle::new();
		self.unsigned_proposals_tx = Some(unsigned_proposals_tx);

		let params = AsyncProtocolParameters {
			blockchain_iface: Arc::new(DKGIface {
				backend: self.backend.clone(),
				latest_header: self.latest_header.clone(),
				client: self.client.clone(),
				keystore: self.key_store.clone(),
				gossip_engine: self.gossip_engine.clone(),
				aggregated_public_keys: self.aggregated_public_keys.clone(),
				best_authorities: best_authorities.clone(),
				authority_public_key: authority_public_key.clone(),
				status: status_handle.clone(),
				vote_results: Arc::new(Default::default()),
				_pd: Default::default(),
			}),
			keystore: self.key_store.clone(),
			signed_message_broadcast_handle: self.to_async_proto.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities,
			authority_public_key,
			unsigned_proposals_rx: Arc::new(Mutex::new(Some(unsigned_proposals_rx))),
		};

		if current {
			self.rounds = Some(status_handle)
		} else {
			self.next_rounds = Some(status_handle)
		}

		params
	}
	/// Rotates the contents of the DKG local key files from the queued file to the active file.
	///
	/// This is meant to be used when rotating the DKG. During a rotation, we begin generating
	/// a new queued DKG local key for the new queued authority set. The previously queued set
	/// is now the active set and so we transition their local key data into the active file path.
	///
	/// `DKG_LOCAL_KEY_FILE` - the active file path for the active local key (DKG public key)
	/// `QUEUED_DKG_LOCAL_KEY_FILE` - the queued file path for the queued local key (DKG public key)
	///
	/// This should never execute unless we are certain that the rotation will succeed, i.e.
	/// that the signature of the next DKG public key has been created and stored on-chain.
	fn rotate_local_key_files(&mut self) {
		let mut local_key_path: Option<PathBuf> = None;
		let mut queued_local_key_path: Option<PathBuf> = None;

		if self.base_path.is_some() {
			local_key_path = get_key_path(&self.base_path, DKG_LOCAL_KEY_FILE);
			queued_local_key_path = get_key_path(&self.base_path, QUEUED_DKG_LOCAL_KEY_FILE);
		}

		if let (Some(path), Some(queued_path)) = (local_key_path, queued_local_key_path) {
			if let Err(err) = std::fs::copy(queued_path, path) {
				error!("Error copying queued key {:?}", &err);
			} else {
				debug!("Successfully copied queued key to current key");
			}
		}
	}

	/// Get the signature threshold at a specific block
	pub fn get_signature_threshold(&self, header: &B::Header) -> u16 {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().signature_threshold(&at).unwrap_or_default()
	}

	/// Get the keygen threshold at a specific block
	pub fn get_keygen_threshold(&self, header: &B::Header) -> u16 {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().keygen_threshold(&at).unwrap_or_default()
	}

	/// Get the next signature threshold at a specific block
	pub fn get_next_signature_threshold(&self, header: &B::Header) -> u16 {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().next_signature_threshold(&at).unwrap_or_default()
	}

	/// Get the next keygen threshold at a specific block
	pub fn get_next_keygen_threshold(&self, header: &B::Header) -> u16 {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().next_keygen_threshold(&at).unwrap_or_default()
	}

	/// Get the active DKG public key
	pub fn get_dkg_pub_key(&self, header: &B::Header) -> (AuthoritySetId, Vec<u8>) {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().dkg_pub_key(&at).ok().unwrap_or_default()
	}

	/// Get the next DKG public key
	#[allow(dead_code)]
	pub fn get_next_dkg_pub_key(&self, header: &B::Header) -> Option<(AuthoritySetId, Vec<u8>)> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().next_dkg_pub_key(&at).ok().unwrap_or_default()
	}

	/// Get the jailed keygen authorities
	#[allow(dead_code)]
	pub fn get_keygen_jailed(&self, header: &B::Header, set: &[AuthorityId]) -> Vec<AuthorityId> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self
			.client
			.runtime_api()
			.get_keygen_jailed(&at, set.to_vec())
			.unwrap_or_default()
	}

	/// Get the jailed signing authorities
	pub fn get_signing_jailed(&self, header: &B::Header, set: &[AuthorityId]) -> Vec<AuthorityId> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self
			.client
			.runtime_api()
			.get_signing_jailed(&at, set.to_vec())
			.unwrap_or_default()
	}

	/// Get the best authorities for keygen
	pub fn get_best_authorities(&self, header: &B::Header) -> Vec<(u16, AuthorityId)> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().get_best_authorities(&at).unwrap_or_default()
	}

	/// Get the next best authorities for keygen
	pub fn get_next_best_authorities(&self, header: &B::Header) -> Vec<(u16, AuthorityId)> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().get_next_best_authorities(&at).unwrap_or_default()
	}

	/// Get the party index of our worker
	///
	/// Returns `None` if we are not in the best authority set
	pub fn get_party_index(&mut self, header: &B::Header) -> Option<u16> {
		let public = self.get_authority_public_key();
		let best_authorities = self.get_best_authorities(header);
		for elt in best_authorities {
			if elt.1 == public {
				return Some(elt.0)
			}
		}

		None
	}

	/// Get the next party index of our worker for possible queued keygen
	///
	/// Returns `None` if we are not in the next best authority set
	pub fn get_next_party_index(&mut self, header: &B::Header) -> Option<u16> {
		let public = self.get_authority_public_key();
		let next_best_authorities = self.get_next_best_authorities(header);
		for elt in next_best_authorities {
			if elt.1 == public {
				return Some(elt.0)
			}
		}

		None
	}

	/// Gets latest block number from latest block header
	pub fn get_latest_block_number(&self) -> NumberFor<B> {
		if let Some(latest_header) = self.latest_header.read().clone() {
			*latest_header.number()
		} else {
			NumberFor::<B>::from(0u32)
		}
	}

	/// Gets the active DKG authority key
	pub fn get_authority_public_key(&mut self) -> Public {
		self.key_store
			.authority_id(&self.key_store.public_keys().unwrap())
			.unwrap_or_else(|| panic!("Halp"))
	}

	/// Gets the active Sr25519 public key
	pub fn get_sr25519_public_key(&self) -> sp_core::sr25519::Public {
		self.key_store
			.sr25519_public_key(&self.key_store.sr25519_public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"))
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
			let current_authority_set = self.client.runtime_api().authority_set(&at).ok();
			let queued_authority_set = self.client.runtime_api().queued_authority_set(&at).ok();
			match (current_authority_set, queued_authority_set) {
				(Some(current), Some(queued)) => Some((current, queued)),
				_ => None,
			}
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

	/// Gets the best authorities by authority keys using on-chain reputations
	fn get_best_authority_keys(
		&self,
		header: &B::Header,
		_authority_set: AuthoritySet<Public>,
		active: bool,
	) -> Vec<Public> {
		// Get active threshold or get the next threshold
		let _threshold = if active {
			self.get_keygen_threshold(header)
		} else {
			self.get_next_keygen_threshold(header)
		};
		// Best authorities are taken from the on-chain reputations
		let best_authorities: Vec<Public> =
			self.get_best_authorities(header).iter().map(|(_, key)| key.clone()).collect();

		best_authorities
	}

	fn handle_genesis_dkg_setup(
		&mut self,
		header: &B::Header,
		genesis_authority_set: AuthoritySet<Public>,
	) {
		// Check if the authority set is empty or if this authority set isn't actually the genesis
		// set
		if genesis_authority_set.authorities.is_empty() {
			return
		}
		// If the rounds is none and we are not using the genesis authority set ID
		// there is a critical error. I'm not sure how this can happen but it should
		// prevent an edge case.
		if self.rounds.is_none() && genesis_authority_set.id != GENESIS_AUTHORITY_SET_ID {
			error!(target: "dkg", "🕸️  Rounds is not and authority set is not genesis set ID 0");
			return
		}

		// Check if we've already set up the DKG for this authority set
		if !self.rounds.as_ref().map(|r| r.is_running()).unwrap_or(false)  {
			debug!(target: "dkg", "🕸️  Rounds exists and is active");
			return
		}

		let mut local_key_path: Option<PathBuf> = None;
		if self.base_path.is_some() {
			local_key_path = get_key_path(&self.base_path, DKG_LOCAL_KEY_FILE);
			let _ = cleanup(local_key_path.as_ref().unwrap().clone());
		}

		let latest_block_num = self.get_latest_block_number();

		// DKG keygen authorities are always taken from the best set of authorities
		let round_id = genesis_authority_set.id;
		let maybe_party_index = self.get_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg", "🕸️  NOT IN THE SET OF BEST GENESIS AUTHORITIES: round {:?}", round_id);
			self.rounds = None;
			return
		} else {
			info!(target: "dkg", "🕸️  IN THE SET OF BEST GENESIS AUTHORITIES: round {:?}", round_id);
		}

		let best_authorities: Vec<Public> =
			self.get_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_signature_threshold(header);

		self.dkg_state.listening_for_active_pub_key = true;
		if let Some(rounds) = self.rounds.as_mut() {
			match rounds.start_keygen(latest_block_num) {
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
		// Check if the authority set is empty, return or proceed
		if queued.authorities.is_empty() {
			return
		}

		// Check if the next rounds exists and has processed for this next queued round id
		if self.next_rounds.is_some() && !self.next_rounds.as_ref().unwrap().has_stalled() {
			return
		}

		let mut queued_local_key_path: Option<PathBuf> = None;

		if self.base_path.is_some() {
			queued_local_key_path = get_key_path(&self.base_path, QUEUED_DKG_LOCAL_KEY_FILE);
			let _ = cleanup(queued_local_key_path.as_ref().unwrap().clone());
		}
		let latest_block_num = self.get_latest_block_number();

		// Get the best next authorities using the keygen threshold
		let round_id = queued.id;
		let maybe_party_index = self.get_next_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg", "🕸️  NOT IN THE SET OF BEST NEXT AUTHORITIES: round {:?}", round_id);
			self.next_rounds = None;
			return
		} else {
			info!(target: "dkg", "🕸️  IN THE SET OF BEST NEXT AUTHORITIES: round {:?}", round_id);
		}

		let best_authorities: Vec<Public> =
			self.get_next_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_next_signature_threshold(header);
		/*self.next_rounds = Some(
			MultiPartyECDSARounds::builder()
				.round_id(round_id)
				.party_index(maybe_party_index.unwrap())
				.threshold(self.get_next_signature_threshold(header))
				.parties(self.get_next_keygen_threshold(header))
				.local_key_path(queued_local_key_path)
				.authorities(best_authorities.clone())
				.jailed_signers(self.get_signing_jailed(header, &best_authorities))
				.build(),
		);*/

		let authority_public_key = best_authorities.get_authority_key();
		let async_proto_params = self.generate_async_proto_params(best_authorities, authority_public_key, false);
		match MetaDKGMessageHandler::execute(async_proto_params, threshold) {
			Ok(meta_handler) => {
				let task = async move {
					match meta_handler.await {
						Ok(_) => {
							log::info!(target: "dkg", "The meta handler has executed successfully");
						}

						Err(err) => {
							error!(target: "dkg", "Error executing meta handler {:?}", &err);
							// TODO: pass to dkg error handler channel
						}
					}
				};

				// spawn on parallel thread
				let _handle = tokio::task::spawn(task);
			},

			Err(err) => {
				error!(target: "dkg", "Error starting meta handler {:?}", &err);
				self.handle_dkg_error(err);
			}
		}

		self.dkg_state.listening_for_pub_key = true;
		if let Some(rounds) = self.next_rounds.as_mut() {
			match rounds.start_keygen(latest_block_num) {
				Ok(()) => {
					info!(target: "dkg", "Keygen started for queued authority set successfully");
					self.queued_keygen_in_progress = true;
				},
				Err(err) => {

				},
			}
		}
	}

	// *** Block notifications ***
	fn process_block_notification(&mut self, header: &B::Header) {
		if let Some(latest_header) = self.latest_header.read().clone() {
			if latest_header.number() >= header.number() {
				return
			}
		}
		*self.latest_header.write() = Some(header.clone());
		// Clear offchain storage
		listen_and_clear_offchain_storage(self, header);
		// Try to resume DKG when we hear of a new block
		try_resume_dkg(self, header);
		// Attempt to enact new DKG authorities if sessions have changed
		if header.number() <= &NumberFor::<B>::from(1u32) {
			debug!(target: "dkg", "Starting genesis DKG setup");
			self.enact_genesis_authorities(header);
		} else {
			self.enact_new_authorities(header);
		}
		// Send all outgoing messages
		send_outgoing_dkg_messages(self);
		// Get all unsigned proposals, create offline stages, attempt voting.
		// Only do this if the public key is set on-chain.
		if !self.get_dkg_pub_key(header).1.is_empty() {
			self.create_offline_stages(header);
			// Send outgoing messages after offline stage creation
			send_outgoing_dkg_messages(self);
			// Get all unsigned proposals and check if they are ready to be signed.
			self.process_unsigned_proposals(header);
			self.untrack_unsigned_proposals(header);
		} else {
			debug!(target: "dkg", "Public key not set on-chain, not creating offline stages {:?}", self.get_dkg_pub_key(header));
		}
	}

	fn enact_genesis_authorities(&mut self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, queued)) = self.validator_set(header) {
			// If we are in the genesis state, we need to enact the genesis authorities
			if active.id == GENESIS_AUTHORITY_SET_ID && self.best_dkg_block.is_none() {
				debug!(target: "dkg", "🕸️  GENESIS ROUND_ID {:?}", active.id);
				metric_set!(self, dkg_validator_set_id, active.id);
				// Setting new validator set id as current
				*self.current_validator_set.write() = active.clone();
				self.queued_validator_set = queued.clone();
				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				self.best_dkg_block = Some(*header.number());
				// Setting up the DKG
				self.handle_genesis_dkg_setup(header, active);
				// Setting up the queued DKG at genesis
				self.handle_queued_dkg_setup(header, queued);
				// Send outgoing messages after processing the queued DKG setup
				send_outgoing_dkg_messages(self);
			}
		}
	}

	fn enact_new_authorities(&mut self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, queued)) = self.validator_set(header) {
			// If the active rounds have stalled, it means we haven't
			// successfully generate a genesis key yet. Therefore, we
			// continue to re-run keygen.
			if let Some(rounds) = self.rounds.as_mut() {
				if rounds.has_stalled() {
					self.handle_genesis_dkg_setup(header, active.clone());
				}
			}
			// If the next rounds have stalled, we restart similarly to above.
			if let Some(rounds) = self.next_rounds.as_mut() {
				if rounds.has_stalled() {
					self.handle_queued_dkg_setup(header, queued.clone());
				}
			}
			// If the session has changed and a keygen is not in progress, we rotate
			if self.queued_validator_set.id != queued.id && !self.queued_keygen_in_progress {
				debug!(target: "dkg", "🕸️  ACTIVE ROUND_ID {:?}", active.id);
				metric_set!(self, dkg_validator_set_id, active.id);
				// Rotate the queued key file contents into the local key file if the next
				// DKG public key signature has been posted on-chain.
				let (set_id, _) = self.get_dkg_pub_key(header);
				if set_id == queued.id - 1 {
					debug!(target: "dkg", "🕸️  ROTATING LOCAL KEY FILE");
					self.rotate_local_key_files();
					// Rotate the rounds since the authority set has changed
					self.rounds = self.next_rounds.take();
				} else {
					debug!(target: "dkg", "🕸️  WAITING FOR NEXT DKG PUBLIC KEY SIG");
				}

				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				// Update the validator sets
				*self.current_validator_set.write() = active;
				self.queued_validator_set = queued.clone();
				// Start the queued DKG setup for the new queued authorities
				self.handle_queued_dkg_setup(header, queued);
				// Send outgoing messages after processing the queued DKG setup
				send_outgoing_dkg_messages(self);
			}
		}
	}

	fn handle_finality_notification(&mut self, notification: FinalityNotification<B>) {
		trace!(target: "dkg", "🕸️  Finality notification: {:?}", notification);
		// Handle import notifications
		self.process_block_notification(&notification.header);
	}

	fn handle_import_notification(&mut self, notification: BlockImportNotification<B>) {
		trace!(target: "dkg", "🕸️  Finality notification: {:?}", notification);
		// Handle import notification
		self.process_block_notification(&notification.header);
	}

	fn verify_signature_against_authorities(
		&mut self,
		signed_dkg_msg: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError> {
		Self::verify_signature_against_authorities_inner(
			signed_dkg_msg,
			&self.latest_header,
			&self.client,
		)
	}

	pub fn verify_signature_against_authorities_inner(
		signed_dkg_msg: SignedDKGMessage<Public>,
		latest_header: &Arc<RwLock<Option<B::Header>>>,
		client: &Arc<C>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let dkg_msg = signed_dkg_msg.msg;
		let encoded = dkg_msg.encode();
		let signature = signed_dkg_msg.signature.unwrap_or_default();
		// Get authority accounts
		let mut authority_accounts: Option<(Vec<AccountId32>, Vec<AccountId32>)> = None;

		if let Some(ref header) = latest_header.read().clone() {
			let at: BlockId<B> = BlockId::hash(header.hash());
			let accounts = client.runtime_api().get_authority_accounts(&at).ok();

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

		if check_signers(authority_accounts.clone().unwrap().0) ||
			check_signers(authority_accounts.unwrap().1)
		{
			Ok(dkg_msg)
		} else {
			Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority or next authority"
					.into(),
			})
		}
	}

	pub fn handle_dkg_error(&mut self, dkg_error: DKGError) {
		let authorities = self.current_validator_set.read().authorities.clone();

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
		let (offender, round_id, misbehaviour_type) = match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender } => {
				info!(target: "dkg", "🕸️  DKG Keygen misbehaviour by {}", offender);
				if let Some(rounds) = self.next_rounds.as_mut() {
					(offender, rounds.get_id(), MisbehaviourType::Keygen)
				} else {
					(offender, 0, MisbehaviourType::Keygen)
				}
			},
			DKGReport::SigningMisbehaviour { offender } => {
				info!(target: "dkg", "🕸️  DKG Signing misbehaviour by {}", offender);
				if let Some(rounds) = self.rounds.as_mut() {
					(offender, rounds.get_id(), MisbehaviourType::Sign)
				} else {
					(offender, 0, MisbehaviourType::Sign)
				}
			},
		};

		let misbehaviour_msg =
			DKGMisbehaviourMessage { misbehaviour_type, round_id, offender, signature: vec![] };
		let hash = sp_core::blake2_128(&misbehaviour_msg.encode());
		let count = *self.has_sent_gossip_msg.get(&hash).unwrap_or(&0u8);
		if count > GOSSIP_MESSAGE_RESENDING_LIMIT {
			return
		}
		gossip_misbehaviour_report(self, misbehaviour_msg);
		self.has_sent_gossip_msg.insert(hash, count + 1);
	}

	pub fn authenticate_msg_origin(
		&self,
		is_main_round: bool,
		authorities: (Vec<Public>, Vec<Public>),
		msg: &[u8],
		signature: &[u8],
	) -> Result<Public, DKGError> {
		let get_keys = |accts: &[Public]| {
			accts
				.iter()
				.map(|x| {
					ecdsa::Public(to_slice_33(&x.encode()).unwrap_or_else(|| {
						panic!("Failed to convert account id to ecdsa public key")
					}))
				})
				.collect::<Vec<ecdsa::Public>>()
		};

		let maybe_signers =
			if is_main_round { get_keys(&authorities.0) } else { get_keys(&authorities.1) };

		let (maybe_signer, success) = dkg_runtime_primitives::utils::verify_signer_from_set_ecdsa(
			maybe_signers,
			msg,
			signature,
		);

		if !success {
			return Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority".to_string(),
			})
		}

		Ok(Public::from(maybe_signer.unwrap()))
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
			if let Some(prop) = proposal {
				proposals.push(prop)
			};
		}
		metric_set!(
			self,
			dkg_round_concluded,
			self.rounds.as_mut().unwrap().get_finished_rounds().len()
		);

		save_signed_proposals_in_storage(self, proposals);
	}

	fn handle_finished_round(&mut self, finished_round: DKGSignedPayload) -> Option<Proposal> {
		trace!(target: "dkg", "Got finished round {:?}", finished_round);
		let decoded_key = <(TypedChainId, DKGPayloadKey)>::decode(&mut &finished_round.key[..]);
		let payload_key = match decoded_key {
			Ok((_chain_id, key)) => key,
			Err(_err) => return None,
		};

		get_signed_proposal(self, finished_round, payload_key)
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
		// rounds:
		// next_rounds:
		// one is active (gets unsigned proposals). We do NOT send proposals to next, ONLY active
		// queued does not receive anything after being created (STOPS after keygen, then, gets replaced with next)
		// Always send to "active" - NOT the queued
		// genesis: run genesis dkg, AND, next_rounds (keygen) (keep retrying until success)
		// wait til public key on-chain, then, send unsigned proposals to active
		// active received unsigned proposals and signs them
		// then, put signed proposals on-chain
		// Eventually, when the session changes, make next_rounds the active rounds

		// Get rid of function that handles unsigned proposals
		self.unsigned_proposals_tx.unbounded_send(unsigned_proposals)?;

		let rounds = self.rounds.as_mut().unwrap();
		let mut errors = Vec::new();
		if rounds.is_keygen_finished() {
			// Current signers before resetting jailed signers
			let signers_hashset =
				rounds.signers.iter().cloned().collect::<HashSet<u16, RandomState>>();
			// Update jailed signers each time an offline stage is created
			// This will trigger a regeneration of signing parties in hopes of
			// both randomizing the set of signers as well as removing jailed
			// signers after reported signing misbehaviour.
			let at: BlockId<B> = BlockId::hash(header.hash());
			let jailed_signers = self
				.client
				.runtime_api()
				.get_signing_jailed(&at, rounds.authorities.clone())
				.unwrap_or_default();
			// Update the jailed signers which may mutate the signers set
			rounds.set_jailed_signers(jailed_signers);
			let new_signers_hashset =
				rounds.signers.iter().cloned().collect::<HashSet<u16, RandomState>>();
			// Iterate through each unsigned proposal and create offline stages
			for unsigned_proposals in &unsigned_proposals {
				let key = (unsigned_proposals.typed_chain_id, unsigned_proposals.key);
				if self.dkg_state.created_offlinestage_at.contains_key(&key.encode()) {
					continue
				}

				// If there are newly jailed signers we need to ensure we restart the offline stage
				// process
				if signers_hashset.difference(&new_signers_hashset).next().is_some() {
					debug!(target: "dkg", "🕸️  FOUND INTERSECTION, REMOVING OLD OFFLINE STAGE\n{:?}\n{:?}", signers_hashset, new_signers_hashset);
					rounds.offlines.remove(&key.encode());
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
		let mut errors = Vec::new();
		for unsigned_proposal in unsigned_proposals {
			let key = (unsigned_proposal.typed_chain_id, unsigned_proposal.key);
			if !rounds.is_ready_to_vote(key.encode()) {
				continue
			}

			debug!(target: "dkg", "Got unsigned proposal with key = {:?}", &key);
			if let Proposal::Unsigned { kind: _, data } = unsigned_proposal.proposal {
				if let Err(e) = rounds.vote(key.encode(), data, latest_block_num) {
					error!(target: "dkg", "🕸️  error creating new vote: {}", e);
					errors.push(e);
				} else {
					self.votes_sent += 1;
				};
			}
		}
		// Handle all errors
		for e in errors {
			self.handle_dkg_error(e);
		}
		// Votes sent to the DKG are unsigned proposals
		metric_set!(self, dkg_votes_sent, self.votes_sent);

		// Send messages to all peers
		send_outgoing_dkg_messages(self);
	}

	// *** Main run loop ***

	pub(crate) async fn run(mut self) {
		let mut dkg =
			Box::pin(self.gossip_engine.lock().messages_for(dkg_topic::<B>()).filter_map(
				|notification| async move {
					SignedDKGMessage::<Public>::decode(&mut &notification.message[..]).ok()
				},
			));

		let to_async_proto = &self.to_async_proto.clone();

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
				notification = self.import_notifications.next().fuse() => {
					if let Some(notification) = notification {
						self.handle_import_notification(notification);
					} else {
						return;
					}
				},
				dkg_msg = dkg.next().fuse() => {
					if let Some(dkg_msg) = dkg_msg {
						if self.to_async_proto.receiver_count() == 0 || self.current_validator_set.read().authorities.is_empty() || self.queued_validator_set.authorities.is_empty() {
							self.msg_cache.push(dkg_msg);
						} else {
							let msgs = self.msg_cache.drain(..);
							for msg in msgs {
								if let Err(err) = to_async_proto.send(Arc::new(msg)) {
									log::error!(target: "dkg", "Unable to send message to async proto: {:?}", err);
								}
							}

							// send dkg_msg to async_proto
							if let Err(err) = to_async_proto.send(Arc::new(dkg_msg)) {
								log::error!(target: "dkg", "Unable to send message to async proto: {:?}", err);
							}
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
