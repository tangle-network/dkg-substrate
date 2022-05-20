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
use sp_core::ecdsa;
use std::{
	collections::{BTreeSet, HashMap},
	marker::PhantomData,
	path::PathBuf,
	sync::Arc,
};

use codec::{Codec, Decode, Encode};

use futures::{future, FutureExt, StreamExt};
use log::{debug, error, info, trace};

use parking_lot::{Mutex, RwLock};

use sc_client_api::{
	Backend, BlockImportNotification, FinalityNotification, FinalityNotifications,
	ImportNotifications,
};

use sp_api::BlockId;
use sp_runtime::traits::{Block, Header, NumberFor};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::keystore::DKGKeystore;

use crate::messages::misbehaviour_report::{
	gossip_misbehaviour_report, handle_misbehaviour_report,
};

use crate::{
	meta_async_rounds::dkg_gossip_engine::GossipEngineIface,
	storage::clear::listen_and_clear_offchain_storage,
};

use dkg_primitives::{
	types::{DKGError, DKGMisbehaviourMessage, RoundId},
	AuthoritySetId, DKGReport, MisbehaviourType, GOSSIP_MESSAGE_RESENDING_LIMIT,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::to_slice_33,
	AggregatedMisbehaviourReports, AggregatedPublicKeys, GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	error, metric_set,
	metrics::Metrics,
	types::dkg_topic,
	utils::{find_authorities_change, get_key_path},
	Client,
};

use crate::messages::public_key_gossip::handle_public_key_broadcast;
use dkg_primitives::{
	types::{DKGMessage, DKGMsgPayload, SignedDKGMessage},
	utils::{cleanup, DKG_LOCAL_KEY_FILE, QUEUED_DKG_LOCAL_KEY_FILE},
};
use dkg_runtime_primitives::{AuthoritySet, DKGApi};

use crate::meta_async_rounds::{
	blockchain_interface::{BlockChainIface, DKGIface},
	meta_handler::{AsyncProtocolParameters, MetaAsyncProtocolHandler},
	remote::MetaAsyncProtocolRemote,
};

pub const ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

pub const STORAGE_SET_RETRY_NUM: usize = 5;

pub const MAX_SUBMISSION_DELAY: u32 = 3;

pub(crate) struct WorkerParams<B, BE, C, GE>
where
	B: Block,
	GE: GossipEngineIface,
{
	pub client: Arc<C>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: GE,
	pub metrics: Option<Metrics>,
	pub base_path: Option<PathBuf>,
	pub local_keystore: Option<Arc<LocalKeystore>>,
	pub _marker: PhantomData<B>,
}

/// A DKG worker plays the DKG protocol
pub(crate) struct DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	pub client: Arc<C>,
	//pub to_async_proto: tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub gossip_engine: Arc<GE>,
	pub metrics: Option<Metrics>,
	pub rounds: Option<MetaAsyncProtocolRemote<NumberFor<B>>>,
	pub next_rounds: Option<MetaAsyncProtocolRemote<NumberFor<B>>>,
	pub finality_notifications: FinalityNotifications<B>,
	pub import_notifications: ImportNotifications<B>,
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
	/// Local keystore for DKG data
	pub base_path: Option<PathBuf>,
	/// Concrete type that points to the actual local keystore if it exists
	pub local_keystore: Option<Arc<LocalKeystore>>,
	/// For transmitting errors from parallel threads to the DKGWorker
	pub error_handler_tx: UnboundedSender<DKGError>,
	/// For use in the run() function only
	pub error_handler_rx: Option<UnboundedReceiver<DKGError>>,
	// keep rustc happy
	_backend: PhantomData<BE>,
}

impl<B, BE, C, GE> DKGWorker<B, BE, C, GE>
where
	B: Block + Codec,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
{
	/// Return a new DKG worker instance.
	///
	/// Note that a DKG worker is only fully functional if a corresponding
	/// DKG pallet has been deployed on-chain.
	///
	/// The DKG pallet is needed in order to keep track of the DKG authority set.
	pub(crate) fn new(worker_params: WorkerParams<B, BE, C, GE>) -> Self {
		let WorkerParams {
			client,
			backend,
			key_store,
			gossip_engine,
			metrics,
			base_path,
			local_keystore,
			..
		} = worker_params;

		// we drop the rx handle since we can call tx.subscribe() every time we need to fire up
		// a new AsyncProto
		// let (to_async_proto, _) = tokio::sync::broadcast::channel(1024);
		let (error_handler_tx, error_handler_rx) = tokio::sync::mpsc::unbounded_channel();

		DKGWorker {
			client: client.clone(),
			backend,
			key_store,
			gossip_engine: Arc::new(gossip_engine),
			metrics,
			rounds: None,
			next_rounds: None,
			finality_notifications: client.finality_notification_stream(),
			import_notifications: client.import_notification_stream(),
			best_dkg_block: None,
			current_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			queued_validator_set: AuthoritySet::empty(),
			latest_header: Arc::new(RwLock::new(None)),
			msg_cache: Vec::new(),
			aggregated_public_keys: Arc::new(Mutex::new(HashMap::new())),
			aggregated_misbehaviour_reports: HashMap::new(),
			has_sent_gossip_msg: HashMap::new(),
			base_path,
			local_keystore,
			error_handler_tx,
			error_handler_rx: Some(error_handler_rx),
			_backend: PhantomData,
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ProtoStageType {
	Genesis,
	Queued,
}

impl<B, BE, C, GE> DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
{
	// NOTE: This must be ran at the start of each epoch since best_authorities may change
	// if "current" is true, this will set the "rounds" field in the dkg worker, otherwise,
	// it well set the "next_rounds" field
	fn generate_async_proto_params(
		&mut self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		round_id: RoundId,
		local_key_path: Option<PathBuf>,
		stage: ProtoStageType,
	) -> Result<AsyncProtocolParameters<DKGIface<B, BE, C, GE>>, DKGError> {
		let best_authorities = Arc::new(best_authorities);
		let authority_public_key = Arc::new(authority_public_key);

		let now = self.get_latest_block_number();
		let status_handle = MetaAsyncProtocolRemote::new(now, round_id);

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
				current_validator_set: self.current_validator_set.clone(),
				local_keystore: self.local_keystore.clone(),
				vote_results: Arc::new(Default::default()),
				local_key_path,
				is_genesis: stage == ProtoStageType::Genesis,
				_pd: Default::default(),
			}),
			round_id,
			keystore: self.key_store.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities,
			authority_public_key,
			batch_id_gen: Arc::new(Default::default()),
			handle: status_handle.clone(),
		};

		// allow either type to immediately begin keygen
		status_handle.start()?;

		if stage != ProtoStageType::Queued {
			self.rounds = Some(status_handle.into_primary_remote())
		} else {
			self.next_rounds = Some(status_handle.into_primary_remote())
		}

		Ok(params)
	}

	fn spawn_async_protocol(
		&mut self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		round_id: RoundId,
		threshold: u16,
		local_key_path: Option<PathBuf>,
		stage: ProtoStageType,
	) {
		match self.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			round_id,
			local_key_path,
			stage,
		) {
			Ok(async_proto_params) => {
				let err_handler_tx = self.error_handler_tx.clone();

				match MetaAsyncProtocolHandler::setup(async_proto_params, threshold) {
					Ok(meta_handler) => {
						let task = async move {
							match meta_handler.await {
								Ok(_) => {
									log::info!(target: "dkg", "The meta handler has executed successfully");
								},

								Err(err) => {
									error!(target: "dkg", "Error executing meta handler {:?}", &err);
									let _ = err_handler_tx.send(err);
								},
							}
						};

						// spawn on parallel thread
						let _handle = tokio::task::spawn(task);
					},

					Err(err) => {
						error!(target: "dkg", "Error starting meta handler {:?}", &err);
						self.handle_dkg_error(err);
					},
				}
			},

			Err(err) => self.handle_dkg_error(err),
		}
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

	/// Get the next signature threshold at a specific block
	pub fn get_next_signature_threshold(&self, header: &B::Header) -> u16 {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self.client.runtime_api().next_signature_threshold(&at).unwrap_or_default()
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
		Self::validator_set_inner(header, &self.client)
	}

	fn validator_set_inner(
		header: &B::Header,
		client: &Arc<C>,
	) -> Option<(AuthoritySet<Public>, AuthoritySet<Public>)> {
		let new = if let Some((new, queued)) = find_authorities_change::<B>(header) {
			Some((new, queued))
		} else {
			let at: BlockId<B> = BlockId::hash(header.hash());
			let current_authority_set = client.runtime_api().authority_set(&at).ok();
			let queued_authority_set = client.runtime_api().queued_authority_set(&at).ok();
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

		let latest_block_num = self.get_latest_block_number();

		// Check if we've already set up the DKG for this authority set
		// if the active is currently running, and, the keygen has stalled, create one anew
		if let Some(rounds) = self.rounds.as_ref() {
			if rounds.is_active() && !rounds.keygen_has_stalled(latest_block_num) {
				debug!(target: "dkg", "🕸️  Rounds exists and is active");
				return
			}
		}

		let mut local_key_path: Option<PathBuf> = None;
		if self.base_path.is_some() {
			local_key_path = get_key_path(&self.base_path, DKG_LOCAL_KEY_FILE);
			let _ = cleanup(local_key_path.as_ref().unwrap().clone());
		}

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
		let authority_public_key = self.get_authority_public_key();
		self.spawn_async_protocol(
			best_authorities,
			authority_public_key,
			round_id,
			threshold,
			local_key_path,
			ProtoStageType::Genesis,
		);
	}

	fn handle_queued_dkg_setup(&mut self, header: &B::Header, queued: AuthoritySet<Public>) {
		// Check if the authority set is empty, return or proceed
		if queued.authorities.is_empty() {
			return
		}

		// Check if the next rounds exists and has processed for this next queued round id
		if self.next_rounds.is_some() &&
			!self.next_rounds.as_ref().unwrap().keygen_has_stalled(*header.number())
		{
			return
		}

		let mut queued_local_key_path: Option<PathBuf> = None;

		if self.base_path.is_some() {
			queued_local_key_path = get_key_path(&self.base_path, QUEUED_DKG_LOCAL_KEY_FILE);
			let _ = cleanup(queued_local_key_path.as_ref().unwrap().clone());
		}

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

		let authority_public_key = self.get_authority_public_key();
		self.spawn_async_protocol(
			best_authorities,
			authority_public_key,
			round_id,
			threshold,
			queued_local_key_path,
			ProtoStageType::Queued,
		);
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
		// We no longer "resume" the DKG worker since it is running in a parallel thread
		// Attempt to enact new DKG authorities if sessions have changed
		if header.number() <= &NumberFor::<B>::from(1u32) {
			debug!(target: "dkg", "Starting genesis DKG setup");
			self.enact_genesis_authorities(header);
		} else {
			self.enact_new_authorities(header);
		}
		// Send all outgoing messages
		//self.send_outgoing_dkg_messages(self);
		// Get all unsigned proposals, create offline stages, attempt voting.
		// Only do this if the public key is set on-chain.
		if !self.get_dkg_pub_key(header).1.is_empty() {
			self.process_unsigned_proposals(header);
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
				//send_outgoing_dkg_messages(self);
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
				if rounds.keygen_has_stalled(*header.number()) {
					self.handle_genesis_dkg_setup(header, active.clone());
				}
			}
			// If the next rounds have stalled, we restart similarly to above.
			if let Some(rounds) = self.next_rounds.as_mut() {
				if rounds.keygen_has_stalled(*header.number()) {
					self.handle_queued_dkg_setup(header, queued.clone());
				}
			}

			let queued_keygen_in_progress =
				self.next_rounds.as_ref().map(|r| !r.is_keygen_finished()).unwrap_or(false);
			// If the session has changed and a keygen is not in progress, we rotate
			if self.queued_validator_set.id != queued.id && !queued_keygen_in_progress {
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
				//send_outgoing_dkg_messages(self);
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
		let signature = signed_dkg_msg.signature.unwrap();
		// Get authority accounts
		let mut authorities: Option<(Vec<AuthorityId>, Vec<AuthorityId>)> = None;
		if let Some(header) = latest_header.read().clone() {
			authorities = Self::validator_set_inner(&header, client)
				.map(|a| (a.0.authorities, a.1.authorities));
		}

		if authorities.is_none() {
			return Err(DKGError::GenericError { reason: "No authorities".into() })
		}

		let check_signers = |xs: &[AuthorityId]| {
			return dkg_runtime_primitives::utils::verify_signer_from_set_ecdsa(
				xs.iter()
					.map(|x| ecdsa::Public::from_raw(to_slice_33(&x.encode()).unwrap()))
					.collect(),
				&encoded,
				&signature,
			)
			.1
		};

		if check_signers(&authorities.clone().unwrap().0) || check_signers(&authorities.unwrap().1)
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
		log::error!(target: "dkg", "Received error: {:?}", dkg_error);
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

	/// Route messages internally where they need to be routed
	fn process_incoming_dkg_message(&mut self, dkg_msg: SignedDKGMessage<Public>) {
		match &dkg_msg.msg.payload {
			DKGMsgPayload::Keygen(..) | DKGMsgPayload::Offline(..) | DKGMsgPayload::Vote(..) => {
				let is_keygen_type = matches!(&dkg_msg.msg.payload, DKGMsgPayload::Keygen(..));
				let msg = Arc::new(dkg_msg);
				if let Some(rounds) = self.rounds.as_mut() {
					// route to async proto
					if let Err(err) = rounds.deliver_message(msg.clone()) {
						self.handle_dkg_error(DKGError::CriticalError { reason: err.to_string() })
					}
				}

				// only route keygen types to the next_rounds
				if is_keygen_type {
					if let Some(rounds) = self.next_rounds.as_mut() {
						// route to async proto
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
					}
				}
			},
			DKGMsgPayload::PublicKeyBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg) {
					Ok(dkg_msg) => {
						match handle_public_key_broadcast(self, dkg_msg) {
							Ok(()) => (),
							Err(err) =>
								error!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
						};
					},

					Err(err) => {
						log::error!(target: "dkg", "Error while verifying signature against authorities: {:?}", err)
					},
				}
			},
			DKGMsgPayload::MisbehaviourBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg) {
					Ok(dkg_msg) => {
						match handle_misbehaviour_report(self, dkg_msg) {
							Ok(()) => (),
							Err(err) =>
								error!(target: "dkg", "🕸️  Error while handling DKG message {:?}", err),
						};
					},

					Err(err) => {
						log::error!(target: "dkg", "Error while verifying signature against authorities: {:?}", err)
					},
				}
			},
		}
	}

	fn handle_dkg_report(&mut self, dkg_report: DKGReport) {
		let (offender, round_id, misbehaviour_type) = match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender } => {
				info!(target: "dkg", "🕸️  DKG Keygen misbehaviour by {}", offender);
				if let Some(rounds) = self.next_rounds.as_mut() {
					(offender, rounds.round_id, MisbehaviourType::Keygen)
				} else {
					(offender, 0, MisbehaviourType::Keygen)
				}
			},
			DKGReport::SigningMisbehaviour { offender } => {
				info!(target: "dkg", "🕸️  DKG Signing misbehaviour by {}", offender);
				if let Some(rounds) = self.rounds.as_mut() {
					(offender, rounds.round_id, MisbehaviourType::Sign)
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

	fn submit_unsigned_proposals(&mut self, header: &B::Header) {
		if let Some(rounds) = self.rounds.as_ref() {
			let at: BlockId<B> = BlockId::hash(header.hash());
			let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
				Ok(res) => res,
				Err(_) => return,
			};

			debug!(target: "dkg", "Got unsigned proposals count {}", unsigned_proposals.len());

			// Get rid of function that handles unsigned proposals
			if let Err(err) = rounds.submit_unsigned_proposals(unsigned_proposals) {
				self.handle_dkg_error(DKGError::CreateOfflineStage { reason: err.to_string() })
			}
		}
	}

	fn process_unsigned_proposals(&mut self, header: &B::Header) {
		self.submit_unsigned_proposals(header);
		//send_outgoing_dkg_messages(self);
	}

	// *** Main run loop ***

	pub(crate) async fn run(mut self) {
		let mut dkg = self.gossip_engine.stream();

		let mut error_handler_rx = self.error_handler_rx.take().unwrap();

		loop {
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

				error = error_handler_rx.recv().fuse() => {
					if let Some(error) = error {
						self.handle_dkg_error(error)
					} else {
						return;
					}
				},

				dkg_msg = dkg.next().fuse() => {
					if let Some(dkg_msg) = dkg_msg {
						if self.rounds.is_none() || self.current_validator_set.read().authorities.is_empty() || self.queued_validator_set.authorities.is_empty() {
							self.msg_cache.push(dkg_msg);
						} else {
							let msgs = self.msg_cache.drain(..).collect::<Vec<_>>();
							for msg in msgs {
								self.process_incoming_dkg_message(msg);
							}

							// send dkg_msg to async_proto
							self.process_incoming_dkg_message(dkg_msg);
						}
					} else {
						return;
					}
				},
			}
		}
	}
}

/// Extension trait for any type that contains a keystore
pub trait KeystoreExt {
	fn get_keystore(&self) -> &DKGKeystore;
	fn get_authority_public_key(&self) -> Public {
		self.get_keystore()
			.authority_id(&self.get_keystore().public_keys().unwrap())
			.unwrap_or_else(|| panic!("Could not find authority public key"))
	}

	fn get_sr25519_public_key(&self) -> sp_core::sr25519::Public {
		self.get_keystore()
			.sr25519_public_key(&self.get_keystore().sr25519_public_keys().unwrap_or_default())
			.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"))
	}
}

impl<B, BE, C, GE> KeystoreExt for DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
{
	fn get_keystore(&self) -> &DKGKeystore {
		&self.key_store
	}
}

impl<B: Block, BE, C, GE> KeystoreExt for DKGIface<B, BE, C, GE> {
	fn get_keystore(&self) -> &DKGKeystore {
		&self.keystore
	}
}

impl<T: BlockChainIface> KeystoreExt for AsyncProtocolParameters<T> {
	fn get_keystore(&self) -> &DKGKeystore {
		&self.keystore
	}
}

impl KeystoreExt for DKGKeystore {
	fn get_keystore(&self) -> &DKGKeystore {
		self
	}
}
