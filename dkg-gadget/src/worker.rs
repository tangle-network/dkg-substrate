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

use crate::{
	async_protocols::blockchain_interface::DKGProtocolEngine, utils::convert_u16_vec_to_usize_vec,
};
use codec::{Codec, Encode};
use dkg_primitives::utils::select_random_set;
use dkg_runtime_primitives::KEYGEN_TIMEOUT;
use futures::{FutureExt, StreamExt};
use itertools::Itertools;
use log::{debug, error, info, trace};
use sc_keystore::LocalKeystore;
use sp_core::ecdsa;
use std::{
	collections::{BTreeSet, HashMap, HashSet},
	future::Future,
	marker::PhantomData,
	path::PathBuf,
	pin::Pin,
	sync::Arc,
};

use parking_lot::{Mutex, RwLock};

use sc_client_api::{
	Backend, BlockImportNotification, FinalityNotification, FinalityNotifications,
	ImportNotifications,
};

use sp_api::BlockId;
use sp_runtime::traits::{Block, Header, NumberFor};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::keystore::DKGKeystore;

use crate::gossip_messages::misbehaviour_report::{
	gossip_misbehaviour_report, handle_misbehaviour_report,
};

use crate::{gossip_engine::GossipEngineIface, storage::clear::listen_and_clear_offchain_storage};

use dkg_primitives::{
	types::{DKGError, DKGMisbehaviourMessage, DKGMsgStatus, RoundId},
	utils::StoredLocalKey,
	AuthoritySetId, DKGReport, MisbehaviourType, GOSSIP_MESSAGE_RESENDING_LIMIT,
};

use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::to_slice_33,
	AggregatedMisbehaviourReports, AggregatedPublicKeys, UnsignedProposal,
	GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	error, metric_set,
	metrics::Metrics,
	persistence::load_stored_key,
	utils::{find_authorities_change, get_key_path},
	Client,
};

use crate::gossip_messages::public_key_gossip::handle_public_key_broadcast;
use dkg_primitives::{
	types::{DKGMessage, DKGMsgPayload, SignedDKGMessage},
	utils::{cleanup, DKG_LOCAL_KEY_FILE, QUEUED_DKG_LOCAL_KEY_FILE},
};
use dkg_runtime_primitives::{AuthoritySet, DKGApi};

use crate::async_protocols::{
	remote::AsyncProtocolRemote, AsyncProtocolParameters, GenericAsyncHandler,
};

pub const ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

pub const STORAGE_SET_RETRY_NUM: usize = 5;

pub const MAX_SUBMISSION_DELAY: u32 = 3;

pub const MAX_SIGNING_SETS: u64 = 16;

pub type Shared<T> = Arc<RwLock<T>>;

pub(crate) struct WorkerParams<B, BE, C, GE>
where
	B: Block,
	GE: GossipEngineIface,
{
	pub client: Arc<C>,
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub keygen_gossip_engine: GE,
	pub signing_gossip_engine: GE,
	pub metrics: Option<Metrics>,
	pub base_path: Option<PathBuf>,
	pub local_keystore: Option<Arc<LocalKeystore>>,
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
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
	pub backend: Arc<BE>,
	pub key_store: DKGKeystore,
	pub keygen_gossip_engine: Arc<GE>,
	pub signing_gossip_engine: Arc<GE>,
	pub metrics: Arc<Option<Metrics>>,
	// Genesis keygen and rotated round
	pub rounds: Shared<Option<AsyncProtocolRemote<NumberFor<B>>>>,
	// Next keygen round, always taken and restarted each session
	pub next_rounds: Shared<Option<AsyncProtocolRemote<NumberFor<B>>>>,
	// Signing rounds, created everytime there are unique unsigned proposals
	pub signing_rounds: Shared<Vec<Option<AsyncProtocolRemote<NumberFor<B>>>>>,
	pub finality_notifications: FinalityNotifications<B>,
	pub import_notifications: ImportNotifications<B>,
	/// Best block a DKG voting round has been concluded for
	pub best_dkg_block: Shared<Option<NumberFor<B>>>,
	/// Cached best authorities
	pub best_authorities: Shared<Vec<(u16, Public)>>,
	/// Cached next best authorities
	pub best_next_authorities: Shared<Vec<(u16, Public)>>,
	/// Latest block header
	pub latest_header: Shared<Option<B::Header>>,
	/// Current validator set
	pub current_validator_set: Shared<AuthoritySet<Public>>,
	/// Queued validator set
	pub queued_validator_set: Shared<AuthoritySet<Public>>,
	/// Tracking for the broadcasted public keys and signatures
	pub aggregated_public_keys: Shared<HashMap<RoundId, AggregatedPublicKeys>>,
	/// Tracking for the misbehaviour reports
	pub aggregated_misbehaviour_reports: Shared<AggregatedMisbehaviourReportStore>,
	pub misbehaviour_tx: Option<UnboundedSender<DKGMisbehaviourMessage>>,
	/// Tracking for sent gossip messages: using blake2_128 for message hashes
	/// The value is the number of times the message has been sent.
	pub has_sent_gossip_msg: Shared<HashMap<[u8; 16], u8>>,
	/// Local keystore for DKG data
	pub base_path: Shared<Option<PathBuf>>,
	/// Concrete type that points to the actual local keystore if it exists
	pub local_keystore: Shared<Option<Arc<LocalKeystore>>>,
	/// For transmitting errors from parallel threads to the DKGWorker
	pub error_handler_tx: UnboundedSender<DKGError>,
	/// For use in the run() function only
	pub error_handler_rx: Shared<Option<UnboundedReceiver<DKGError>>>,
	// keep rustc happy
	_backend: PhantomData<BE>,
}

pub type AggregatedMisbehaviourReportStore =
	HashMap<(MisbehaviourType, RoundId, AuthorityId), AggregatedMisbehaviourReports<AuthorityId>>;

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
			keygen_gossip_engine,
			signing_gossip_engine,
			metrics,
			base_path,
			local_keystore,
			latest_header,
			..
		} = worker_params;

		// we drop the rx handle since we can call tx.subscribe() every time we need to fire up
		// a new AsyncProto
		// let (to_async_proto, _) = tokio::sync::broadcast::channel(1024);
		let (error_handler_tx, error_handler_rx) = tokio::sync::mpsc::unbounded_channel();

		DKGWorker {
			client: client.clone(),
			misbehaviour_tx: None,
			backend,
			key_store,
			keygen_gossip_engine: Arc::new(keygen_gossip_engine),
			signing_gossip_engine: Arc::new(signing_gossip_engine),
			metrics: Arc::new(metrics),
			rounds: Arc::new(RwLock::new(None)),
			next_rounds: Arc::new(RwLock::new(None)),
			signing_rounds: Arc::new(RwLock::new(vec![None; 16])),
			finality_notifications: client.finality_notification_stream(),
			import_notifications: client.import_notification_stream(),
			best_dkg_block: Arc::new(RwLock::new(None)),
			best_authorities: Arc::new(RwLock::new(vec![])),
			best_next_authorities: Arc::new(RwLock::new(vec![])),
			current_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			queued_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			latest_header,
			aggregated_public_keys: Arc::new(RwLock::new(HashMap::new())),
			aggregated_misbehaviour_reports: Arc::new(RwLock::new(HashMap::new())),
			has_sent_gossip_msg: Arc::new(RwLock::new(HashMap::new())),
			base_path: Arc::new(RwLock::new(base_path)),
			local_keystore: Arc::new(RwLock::new(local_keystore)),
			error_handler_tx,
			error_handler_rx: Arc::new(RwLock::new(Some(error_handler_rx))),
			_backend: PhantomData,
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ProtoStageType {
	Genesis,
	Queued,
	Signing,
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
	#[allow(clippy::too_many_arguments, clippy::type_complexity)]
	fn generate_async_proto_params(
		&self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		round_id: RoundId,
		local_key_path: Option<PathBuf>,
		stage: ProtoStageType,
		async_index: u8,
		protocol_name: &str,
	) -> Result<AsyncProtocolParameters<DKGProtocolEngine<B, BE, C, GE>>, DKGError> {
		let best_authorities = Arc::new(best_authorities);
		let authority_public_key = Arc::new(authority_public_key);

		let now = self.get_latest_block_number();
		let status_handle = AsyncProtocolRemote::new(now, round_id);
		// Fetch the active key. This requires rotating the key to have happened with
		// full certainty in order to ensure the right key is being used to make signatures.
		let (active_local_key, _) = self.fetch_local_keys();
		let params = AsyncProtocolParameters {
			engine: Arc::new(DKGProtocolEngine {
				backend: self.backend.clone(),
				latest_header: self.latest_header.clone(),
				client: self.client.clone(),
				keystore: self.key_store.clone(),
				gossip_engine: self.get_gossip_engine_from_protocol_name(protocol_name),
				aggregated_public_keys: self.aggregated_public_keys.clone(),
				best_authorities: best_authorities.clone(),
				authority_public_key: authority_public_key.clone(),
				current_validator_set: self.current_validator_set.clone(),
				local_keystore: self.local_keystore.clone(),
				vote_results: Arc::new(Default::default()),
				local_key_path: Arc::new(RwLock::new(local_key_path)),
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
			local_key: active_local_key.map(|k| k.local_key),
		};
		// Start the respective protocol
		status_handle.start()?;
		// Cache the rounds, respectively
		match stage {
			ProtoStageType::Genesis => {
				let mut lock = self.rounds.write();
				debug!(target: "dkg_gadget::worker", "Starting genesis protocol");
				if lock.is_some() {
					log::warn!(target: "dkg_gadget::worker", "Overwriting rounds will result in termination of previous rounds!");
				}
				*lock = Some(status_handle.into_primary_remote())
			},
			ProtoStageType::Queued => {
				let mut lock = self.next_rounds.write();
				debug!(target: "dkg_gadget::worker", "Starting queued protocol");
				if lock.is_some() {
					log::warn!(target: "dkg_gadget::worker", "Overwriting rounds will result in termination of previous rounds!");
				}
				*lock = Some(status_handle.into_primary_remote())
			},
			// When we are at signing stage, it is using the active rounds.
			ProtoStageType::Signing => {
				debug!(target: "dkg_gadget::worker", "Starting signing protocol: async_index #{}", async_index);
				let mut lock = self.signing_rounds.write();
				lock[async_index as usize] = Some(status_handle.into_primary_remote())
			},
		}

		Ok(params)
	}

	/// Returns the gossip engine based on the protocol_name
	fn get_gossip_engine_from_protocol_name(&self, protocol_name: &str) -> Arc<GE> {
		match protocol_name {
			crate::DKG_KEYGEN_PROTOCOL_NAME => self.keygen_gossip_engine.clone(),
			crate::DKG_SIGNING_PROTOCOL_NAME => self.signing_gossip_engine.clone(),
			_ => panic!("Protocol name not found!"),
		}
	}

	fn spawn_keygen_protocol(
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
			0u8,
			crate::DKG_KEYGEN_PROTOCOL_NAME,
		) {
			Ok(async_proto_params) => {
				let err_handler_tx = self.error_handler_tx.clone();
				let status = if let Some(rounds) = self.rounds.read().as_ref() {
					if rounds.round_id == round_id {
						DKGMsgStatus::ACTIVE
					} else {
						DKGMsgStatus::QUEUED
					}
				} else {
					DKGMsgStatus::UNKNOWN
				};
				match GenericAsyncHandler::setup_keygen(async_proto_params, threshold, status) {
					Ok(meta_handler) => {
						let task = async move {
							match meta_handler.await {
								Ok(_) => {
									log::info!(target: "dkg_gadget::worker", "The meta handler has executed successfully");
								},

								Err(err) => {
									error!(target: "dkg_gadget::worker", "Error executing meta handler {:?}", &err);
									let _ = err_handler_tx.send(err);
								},
							}
						};

						// spawn on parallel thread
						let _handle = tokio::task::spawn(task);
					},

					Err(err) => {
						error!(target: "dkg_gadget::worker", "Error starting meta handler {:?}", &err);
						self.handle_dkg_error(err);
					},
				}
			},

			Err(err) => {
				self.handle_dkg_error(err);
			},
		}
	}

	#[allow(clippy::too_many_arguments, clippy::type_complexity)]
	fn create_signing_protocol(
		&mut self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		round_id: RoundId,
		threshold: u16,
		local_key_path: Option<PathBuf>,
		stage: ProtoStageType,
		unsigned_proposals: Vec<UnsignedProposal>,
		signing_set: Vec<u16>,
		async_index: u8,
	) -> Result<Pin<Box<dyn Future<Output = Result<u8, DKGError>> + Send + 'static>>, DKGError> {
		let async_proto_params = self.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			round_id,
			local_key_path,
			stage,
			async_index,
			crate::DKG_SIGNING_PROTOCOL_NAME,
		)?;

		let err_handler_tx = self.error_handler_tx.clone();
		let meta_handler = GenericAsyncHandler::setup_signing(
			async_proto_params,
			threshold,
			unsigned_proposals,
			signing_set,
			async_index,
		)?;

		let task = async move {
			match meta_handler.await {
				Ok(_) => {
					log::info!(target: "dkg_gadget::worker", "The meta handler has executed successfully");
					Ok(async_index)
				},

				Err(err) => {
					error!(target: "dkg_gadget::worker", "Error executing meta handler {:?}", &err);
					let _ = err_handler_tx.send(err.clone());
					Err(err)
				},
			}
		};

		Ok(Box::pin(task))
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
	fn rotate_local_key_files(&mut self) -> bool {
		let local_key_path = get_key_path(&self.base_path.read().clone(), DKG_LOCAL_KEY_FILE);
		let queued_local_key_path =
			get_key_path(&self.base_path.read().clone(), QUEUED_DKG_LOCAL_KEY_FILE);
		debug!(target: "dkg_gadget::worker", "Rotating local key files");
		debug!(target: "dkg_gadget::worker", "Local key path: {:?}", local_key_path);
		debug!(target: "dkg_gadget::worker", "Queued local key path: {:?}", queued_local_key_path);
		if let (Some(path), Some(queued_path)) = (local_key_path, queued_local_key_path) {
			if let Err(err) = std::fs::copy(queued_path, path) {
				error!("Error copying queued key {:?}", &err);
				return false
			} else {
				debug!("Successfully copied queued key to current key");
				return true
			}
		}

		false
	}

	/// Fetch the local stored keys if they exist.
	fn fetch_local_keys(&mut self) -> (Option<StoredLocalKey>, Option<StoredLocalKey>) {
		let local_key_path = get_key_path(&self.base_path.read().clone(), DKG_LOCAL_KEY_FILE);
		let queued_local_key_path =
			get_key_path(&self.base_path.read().clone(), QUEUED_DKG_LOCAL_KEY_FILE);

		let sr_pub = self.get_sr25519_public_key();
		match (local_key_path, queued_local_key_path) {
			(Some(path), Some(queued_path)) => (
				load_stored_key(path, self.local_keystore.read().as_ref(), sr_pub).ok(),
				load_stored_key(queued_path, self.local_keystore.read().as_ref(), sr_pub).ok(),
			),
			(Some(path), None) =>
				(load_stored_key(path, self.local_keystore.read().as_ref(), sr_pub).ok(), None),
			(None, Some(queued_path)) => (
				None,
				load_stored_key(queued_path, self.local_keystore.read().as_ref(), sr_pub).ok(),
			),
			(None, None) => (None, None),
		}
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

		trace!(target: "dkg_gadget::worker", "🕸️  active validator set: {:?}", new);

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
			debug!(target: "dkg_gadget::worker", "🕸️  for block {:?}, public key missing in validator set is: {:?}", block, missing);
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
		match self.rounds.read().as_ref() {
			None if genesis_authority_set.id != GENESIS_AUTHORITY_SET_ID => {
				error!(target: "dkg_gadget::worker", "🕸️  Rounds is not and authority set is not genesis set ID 0");
				return
			},
			_ => {},
		}

		let latest_block_num = self.get_latest_block_number();

		// Check if we've already set up the DKG for this authority set
		// if the active is currently running, and, the keygen has stalled, create one anew
		match self.rounds.read().as_ref() {
			Some(rounds) if rounds.is_active() && !rounds.keygen_has_stalled(latest_block_num) => {
				debug!(target: "dkg_gadget::worker", "🕸️  Rounds exists and is active");
				return
			},
			_ => {},
		}

		let local_key_path = get_key_path(&self.base_path.read(), DKG_LOCAL_KEY_FILE);
		if let Some(local_key) = local_key_path {
			let _ = cleanup(local_key.clone());
		}
		// DKG keygen authorities are always taken from the best set of authorities
		let round_id = genesis_authority_set.id;
		let maybe_party_index = self.get_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST GENESIS AUTHORITIES: round {:?}", round_id);
			*self.rounds.write() = None;
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST GENESIS AUTHORITIES: round {:?}", round_id);
		}

		let best_authorities: Vec<Public> =
			self.get_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_signature_threshold(header);
		let authority_public_key = self.get_authority_public_key();
		self.spawn_keygen_protocol(
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
			debug!(target: "dkg_gadget::worker", "🕸️  queued authority set is empty");
			return
		}

		// Check if the next rounds exists and has processed for this next queued round id
		match self.next_rounds.read().as_ref() {
			Some(next_rounds) if next_rounds.is_active() => {
				debug!(target: "dkg_gadget::worker", "🕸️  Next rounds exists and is active, returning...");
				return
			},
			_ => {},
		}

		if let Some(rounds) = self.next_rounds.read().as_ref() {
			if rounds.keygen_has_stalled(*header.number()) {
				debug!(target: "dkg_gadget::worker", "🕸️  Next rounds keygen has stalled, creating new rounds...");
			}
		}

		let queued_local_key_path =
			get_key_path(&*self.base_path.read(), QUEUED_DKG_LOCAL_KEY_FILE);
		if let Some(path) = queued_local_key_path.as_ref() {
			let _ = cleanup(path.clone());
		}

		// Get the best next authorities using the keygen threshold
		let round_id = queued.id;
		let maybe_party_index = self.get_next_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST NEXT AUTHORITIES: round {:?}", round_id);
			*self.next_rounds.write() = None;
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST NEXT AUTHORITIES: round {:?}", round_id);
		}

		let best_authorities: Vec<Public> =
			self.get_next_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_next_signature_threshold(header);

		let authority_public_key = self.get_authority_public_key();
		self.spawn_keygen_protocol(
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
				// We've already seen this block, ignore it.
				debug!(target: "dkg_gadget::worker", "🕸️  Latest header is greater than or equal to current header, returning...");
				return
			}
		}
		debug!(target: "dkg_gadget::worker", "🕸️  Processing block notification for block {}", header.number());
		*self.latest_header.write() = Some(header.clone());
		debug!(target: "dkg_gadget::worker", "🕸️  Latest header is now: {:?}", header.number());
		// Clear offchain storage
		listen_and_clear_offchain_storage(self, header);
		// Attempt to enact new DKG authorities if sessions have changed

		// The Steps for enacting new DKG authorities are:
		// 1. Check if the DKG Public Key are not yet set on chain (or not yet generated)
		// 2. if yes, we start enacting authorities on genesis flow.
		// 3. if no, we start enacting authorities on queued flow and submit any unsigned
		//          proposals.
		if self.get_dkg_pub_key(header).1.is_empty() {
			self.maybe_enact_genesis_authorities(header);
		} else {
			self.maybe_enact_new_authorities(header);
			self.submit_unsigned_proposals(header);
		}
	}

	fn maybe_enact_genesis_authorities(&mut self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, queued)) = self.validator_set(header) {
			// If we are in the genesis state, we need to enact the genesis authorities
			if active.id == GENESIS_AUTHORITY_SET_ID && self.best_dkg_block.read().is_none() {
				debug!(target: "dkg_gadget::worker", "🕸️  GENESIS ROUND_ID {:?}", active.id);
				metric_set!(self, dkg_validator_set_id, active.id);
				// Setting new validator set id as current
				*self.current_validator_set.write() = active.clone();
				*self.queued_validator_set.write() = queued.clone();
				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				*self.best_dkg_block.write() = Some(*header.number());
				*self.best_authorities.write() = self.get_best_authorities(header);
				*self.best_next_authorities.write() = self.get_next_best_authorities(header);
				// Setting up the DKG
				self.handle_genesis_dkg_setup(header, active);
				self.handle_queued_dkg_setup(header, queued);
			}
		}
	}

	fn maybe_enact_new_authorities(&mut self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, queued)) = self.validator_set(header) {
			let next_best = self.get_next_best_authorities(header);
			let next_best_has_changed = next_best != *self.best_next_authorities.read();
			if next_best_has_changed && self.queued_validator_set.read().id == queued.id {
				debug!(target: "dkg_gadget::worker", "🕸️  Best authorities has changed on-chain\nOLD {:?}\nNEW: {:?}", self.best_next_authorities, next_best);
				// Update the next best authorities
				*self.best_next_authorities.write() = next_best;
				// Start the queued DKG setup for the new queued authorities
				self.handle_queued_dkg_setup(header, queued);
				return
			}
			// If the next rounds have stalled, we restart similarly to above.
			if let Some(rounds) = self.next_rounds.read().as_ref() {
				debug!(target: "dkg_gadget::worker", "🕸️  Status: {:?}, Now: {:?}, Started At: {:?}, Timeout length: {:?}", rounds.status, header.number(), rounds.started_at, KEYGEN_TIMEOUT);
				if rounds.keygen_is_not_complete() &&
					header.number() >= &(rounds.started_at + KEYGEN_TIMEOUT.into())
				{
					debug!(target: "dkg_gadget::worker", "🕸️  QUEUED DKG STALLED: round {:?}", queued.id);
					return self.handle_dkg_error(DKGError::KeygenTimeout {
						bad_actors: convert_u16_vec_to_usize_vec(
							rounds.current_round_blame().blamed_parties,
						),
					})
				} else {
					debug!(target: "dkg_gadget::worker", "🕸️  QUEUED DKG NOT STALLED: round {:?}", queued.id);
				}
			}

			let queued_keygen_in_progress = self
				.next_rounds
				.read()
				.as_ref()
				.map(|r| !r.is_keygen_finished())
				.unwrap_or(false);

			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED KEYGEN IN PROGRESS: {:?}", queued_keygen_in_progress);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED DKG ID: {:?}", queued.id);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED VALIDATOR SET ID: {:?}", self.queued_validator_set.read().id);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED DKG STATUS: {:?}", self.next_rounds.read().as_ref().map(|r| r.status.clone()));
			// If the session has changed and a keygen is not in progress, we rotate
			if self.queued_validator_set.read().id != queued.id && !queued_keygen_in_progress {
				debug!(target: "dkg_gadget::worker", "🕸️  ACTIVE ROUND_ID {:?}", active.id);
				metric_set!(self, dkg_validator_set_id, active.id);
				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				// Update the validator sets
				*self.current_validator_set.write() = active;
				*self.queued_validator_set.write() = queued.clone();
				// Check the local keystore, if a queued key exists with the same
				// round ID then we shouldn't rotate since it means we have shut down
				// and started up after a previous rotation.
				let (_, maybe_queued_key) = self.fetch_local_keys();
				match maybe_queued_key {
					Some(queued_key) if queued_key.round_id == queued.id => {
						debug!(target: "dkg_gadget::worker", "🕸️  QUEUED KEY EXISTS: {:?}", queued_key.round_id);
						debug!(target: "dkg_gadget::worker", "🕸️  Queued local key exists at same round as queued validator set {:?}", queued.id);
						return
					},
					Some(k) => {
						debug!(target: "dkg_gadget::worker", "🕸️  QUEUED KEY EXISTS: {:?}", k.round_id);
						debug!(target: "dkg_gadget::worker", "🕸️  Queued local key exists at different round than queued validator set {:?}", queued.id);
					},
					None => {
						debug!(target: "dkg_gadget::worker", "🕸️  QUEUED KEY DOES NOT EXIST");
					},
				};
				// If we are starting a new queued DKG, we rotate the next rounds
				log::warn!(target: "dkg_gadget::worker", "🕸️  Rotating next round this will result in a drop/termination of the current rounds!s");
				match self.rounds.as_ref() {
					Some(r) if r.is_active() => {
						log::warn!(target: "dkg_gadget::worker", "🕸️  Current rounds is active, rotating next round will terminate it!!");
					},
					Some(_) | None => {
						log::warn!(target: "dkg_gadget::worker", "🕸️  Current rounds is not active, rotating next rounds is okay");
					},
				};
				self.rounds = self.next_rounds.take();
				// We also rotate the best authority caches
				self.best_authorities = self.best_next_authorities.clone();
				self.best_next_authorities = self.get_next_best_authorities(header);
				// Rotate the key files
				let success = self.rotate_local_key_files();
				debug!(target: "dkg_gadget::worker", "🕸️  ROTATED LOCAL KEY FILES: {:?}", success);
				// Start the queued DKG setup for the new queued authorities
				self.handle_queued_dkg_setup(header, queued);
			}
		} else {
			// no queued validator set, so we don't do anything
			debug!(target: "dkg_gadget::worker", "🕸️  NO QUEUED VALIDATOR SET");
		}
	}

	fn handle_finality_notification(&mut self, notification: FinalityNotification<B>) {
		trace!(target: "dkg_gadget::worker", "🕸️  Finality notification: {:?}", notification);
		// Handle finality notifications
		self.process_block_notification(&notification.header);
	}

	fn handle_import_notification(&mut self, notification: BlockImportNotification<B>) {
		trace!(target: "dkg_gadget::worker", "🕸️  Import notification: {:?}", notification);
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
		log::error!(target: "dkg_gadget", "Received error: {:?}", dkg_error);
		let authorities: Vec<Public> = self.best_authorities.iter().map(|x| x.1.clone()).collect();

		let bad_actors = match dkg_error {
			DKGError::KeygenMisbehaviour { ref bad_actors, .. } => bad_actors.clone(),
			DKGError::KeygenTimeout { ref bad_actors } => bad_actors.clone(),
			DKGError::SignMisbehaviour { ref bad_actors, .. } => bad_actors.clone(),
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
				DKGError::KeygenMisbehaviour { bad_actors: _, .. } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender }),
				DKGError::KeygenTimeout { .. } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender }),
				DKGError::SignMisbehaviour { bad_actors: _, .. } =>
					self.handle_dkg_report(DKGReport::SignMisbehaviour { offender }),
				_ => (),
			}
		}
	}

	/// Route messages internally where they need to be routed
	fn process_incoming_dkg_message(
		&mut self,
		dkg_msg: SignedDKGMessage<Public>,
	) -> Result<(), DKGError> {
		match &dkg_msg.msg.payload {
			DKGMsgPayload::Keygen(..) => {
				let msg = Arc::new(dkg_msg);
				if let Some(rounds) = self.rounds.as_mut() {
					if rounds.round_id == msg.msg.round_id {
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
						return Ok(())
					}
				}

				if let Some(rounds) = self.next_rounds.as_mut() {
					if rounds.round_id == msg.msg.round_id {
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
						return Ok(())
					}
				}

				Err(DKGError::GenericError {
					reason: "Message is not for this DKG round or DKG rounds are not ready yet"
						.into(),
				})
			},
			DKGMsgPayload::Offline(..) | DKGMsgPayload::Vote(..) => {
				let msg = Arc::new(dkg_msg);
				let async_index = msg.msg.payload.get_async_index();
				log::debug!(target: "dkg_gadget::worker", "Received message for async index {}", async_index);
				if let Some(rounds) = self.signing_rounds[async_index as usize].as_mut() {
					log::debug!(target: "dkg_gadget::worker", "Message is for signing round {}", rounds.round_id);
					if rounds.round_id == msg.msg.round_id {
						log::debug!(target: "dkg_gadget::worker", "Message is for this signing round: {}", rounds.round_id);
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
					} else {
						log::warn!(target: "dkg_gadget::worker", "Message is for another signing round: {}", rounds.round_id);
					}
				} else {
					log::warn!(target: "dkg_gadget::worker", "No signing rounds for async index {}", async_index);
				}
				Ok(())
			},
			DKGMsgPayload::PublicKeyBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg) {
					Ok(dkg_msg) => {
						match handle_public_key_broadcast(self, dkg_msg) {
							Ok(()) => (),
							Err(err) =>
								error!(target: "dkg_gadget::worker", "🕸️  Error while handling DKG message {:?}", err),
						};
					},

					Err(err) => {
						log::error!(target: "dkg_gadget::worker", "Error while verifying signature against authorities: {:?}", err)
					},
				}
				Ok(())
			},
			DKGMsgPayload::MisbehaviourBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg) {
					Ok(dkg_msg) => {
						match handle_misbehaviour_report(self, dkg_msg) {
							Ok(()) => (),
							Err(err) =>
								error!(target: "dkg_gadget::worker", "🕸️  Error while handling DKG message {:?}", err),
						};
					},

					Err(err) => {
						log::error!(target: "dkg_gadget::worker", "Error while verifying signature against authorities: {:?}", err)
					},
				}

				Ok(())
			},
		}
	}

	fn handle_dkg_report(&mut self, dkg_report: DKGReport) {
		let (offender, round_id, misbehaviour_type) = match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender } => {
				info!(target: "dkg_gadget::worker", "🕸️  DKG Keygen misbehaviour by {}", offender);
				if let Some(rounds) = self.next_rounds.as_mut() {
					(offender, rounds.round_id, MisbehaviourType::Keygen)
				} else {
					(offender, 0, MisbehaviourType::Keygen)
				}
			},
			DKGReport::SignMisbehaviour { offender } => {
				info!(target: "dkg_gadget::worker", "🕸️  DKG Signing misbehaviour by {}", offender);
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
		self.has_sent_gossip_msg.write().insert(hash, count + 1);
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
		let round_id =
			if let Some(rounds) = self.rounds.read().as_ref() { rounds.round_id } else { return };

		let at: BlockId<B> = BlockId::hash(header.hash());

		let maybe_party_index = self.get_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST AUTHORITIES: round {:?}", round_id);
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST AUTHORITIES: round {:?}", round_id);
		}

		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => res,
			Err(_) => return,
		};

		if unsigned_proposals.is_empty() {
			return
		} else {
			debug!(target: "dkg_gadget::worker", "Got unsigned proposals count {}", unsigned_proposals.len());
		}

		let best_authorities: Vec<Public> =
			self.get_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_signature_threshold(header);
		let authority_public_key = self.get_authority_public_key();

		let (active_local_key, _) = self.fetch_local_keys();
		let local_key = if let Some(active_local_key) = active_local_key {
			active_local_key.local_key
		} else {
			return
		};

		let mut count = 0;
		let mut seed = local_key.public_key().to_bytes(true)[1..].to_vec();

		// Generate multiple signing sets for signing the same unsigned proposals.
		// The goal is to successfully sign proposals immediately in the event that
		// some authorities are not present.
		//
		// For example, if we have authorities: [1,2,3] and we only generate a single
		// signing set (1,2), then if either party is absent, we will not be able to sign
		// until we handle a misbehaviour. Instead, we brute force sign with multiple sets.
		// For `n` authorities, to cover all signing sets of size `t+1`, we need to generate
		// (n choose (t+1)) sets.
		//
		// Sets with the same values are not unique. We only care about all unique, unordered
		// permutations of size `t+1`. i.e. (1,2), (2,3), (1,3) === (2,1), (3,2), (3,1)
		let factorial = |num: u64| match num {
			0 => 1,
			1.. => (1..num + 1).product(),
		};
		// TODO: Modify this to not blow up as n, t grow.
		let mut signing_sets = Vec::new();
		let n = factorial(best_authorities.len() as u64);
		let k = factorial((threshold + 1) as u64);
		let n_minus_k = factorial((best_authorities.len() - threshold as usize - 1) as u64);
		let num_combinations = std::cmp::min(n / (k * n_minus_k), MAX_SIGNING_SETS);
		debug!(target: "dkg_gadget::worker", "Generating {} signing sets", num_combinations);
		while signing_sets.len() < num_combinations as usize {
			if count > 0 {
				seed = sp_core::keccak_256(&seed).to_vec();
			}
			let maybe_set = self.generate_signers(&seed, threshold, best_authorities.clone()).ok();
			if let Some(set) = maybe_set {
				let set = HashSet::<u16>::from_iter(set.iter().cloned());
				if !signing_sets.contains(&set) {
					signing_sets.push(set);
				}
			}

			count += 1;
		}

		let mut futures = Vec::with_capacity(signing_sets.len());
		#[allow(clippy::needless_range_loop)]
		for i in 0..signing_sets.len() {
			// Filter for only the signing sets that contain our party index.
			if signing_sets[i].contains(&maybe_party_index.unwrap()) {
				log::info!(target: "dkg_gadget::worker", "🕸️  Round Id {:?} | Async index {:?} | {}-out-of-{} signers: ({:?})", round_id, i, threshold, best_authorities.len(), signing_sets[i].clone());
				match self.create_signing_protocol(
					best_authorities.clone(),
					authority_public_key.clone(),
					round_id,
					threshold,
					None,
					ProtoStageType::Signing,
					unsigned_proposals.clone(),
					signing_sets[i].clone().into_iter().sorted().collect::<Vec<_>>(),
					i as u8,
				) {
					Ok(task) => futures.push(task),
					Err(err) => {
						log::error!(target: "dkg_gadget::worker", "Error creating signing protocol: {:?}", &err);
						self.handle_dkg_error(err)
					},
				}
			}
		}

		if futures.is_empty() {
			log::error!(target: "dkg_gadget::worker", "While creating the signing protocol, 0 were created");
		} else {
			// the goal of the meta task is to select the first winner
			let meta_signing_protocol = async move {
				// select the first future to return Ok(()), ignoring every failure
				// (note: the errors are not truly ignored since each individual future
				// has logic to handle errors internally, including misbehaviour monitors
				let mut results = futures::future::select_ok(futures).await.into_iter();
				if let Some((_success, _losing_futures)) = results.next() {
					log::info!(target: "dkg_gadget::worker", "*** SUCCESSFULLY EXECUTED meta signing protocol {:?} ***", _success);
				} else {
					log::warn!(target: "dkg_gadget::worker", "*** UNSUCCESSFULLY EXECUTED meta signing protocol");
				}
			};

			// spawn in parallel
			let _handle = tokio::task::spawn(meta_signing_protocol);
		}
	}

	/// After keygen, this should be called to generate a random set of signers
	/// NOTE: since the random set is called using a deterministic seed to and RNG,
	/// the resulting set is deterministic
	fn generate_signers(
		&self,
		seed: &[u8],
		t: u16,
		best_authorities: Vec<Public>,
	) -> Result<Vec<u16>, DKGError> {
		let mut final_set = self.get_unjailed_signers(&best_authorities)?;
		// Mutate the final set if we don't have enough unjailed signers
		if final_set.len() <= t as usize {
			let jailed_set = self.get_jailed_signers(&best_authorities)?;
			let diff = t as usize + 1 - final_set.len();
			final_set = final_set
				.iter()
				.chain(jailed_set.iter().take(diff))
				.cloned()
				.collect::<Vec<u16>>();
		}

		select_random_set(seed, final_set, t + 1).map_err(|err| DKGError::CreateOfflineStage {
			reason: format!("generate_signers failed, reason: {}", err),
		})
	}

	fn get_jailed_signers_inner(
		&self,
		best_authorities: &[Public],
	) -> Result<Vec<Public>, DKGError> {
		let now = self.latest_header.read().clone().ok_or_else(|| DKGError::CriticalError {
			reason: "latest header does not exist!".to_string(),
		})?;
		let at: BlockId<B> = BlockId::hash(now.hash());
		Ok(self
			.client
			.runtime_api()
			.get_signing_jailed(&at, best_authorities.to_vec())
			.unwrap_or_default())
	}
	fn get_unjailed_signers(&self, best_authorities: &[Public]) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner(best_authorities)?;
		Ok(best_authorities
			.iter()
			.enumerate()
			.filter(|(_, key)| !jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}

	/// Get the jailed signers
	fn get_jailed_signers(&self, best_authorities: &[Public]) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner(best_authorities)?;
		Ok(best_authorities
			.iter()
			.enumerate()
			.filter(|(_, key)| jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}

	// *** Main run loop ***

	/// Wait for initial block import
	async fn initialization(&mut self) {
		use futures::future;
		self.client
			.import_notification_stream()
			.take_while(|notif| {
				if let Some((active, queued)) = self.validator_set(&notif.header) {
					// TODO: Consider caching this data and loading it here.
					*self.best_authorities.write() = self.get_best_authorities(&notif.header);
					*self.best_next_authorities.write() =
						self.get_next_best_authorities(&notif.header);
					*self.current_validator_set.write() = active;
					*self.queued_validator_set.write() = queued;
					// Route this to the import notification handler
					self.handle_import_notification(notif.clone());
					log::debug!(target: "dkg_gadget::worker", "Initialization complete");
					future::ready(false)
				} else {
					future::ready(true)
				}
			})
			.for_each(|_| future::ready(()))
			.await;
		// get a new stream that provides _new_ notifications (from here on out)
		self.import_notifications = self.client.import_notification_stream();
	}

	pub(crate) async fn run(mut self) {
		let keygen_gossip_engine = self.keygen_gossip_engine.clone();
		let signing_gossip_engine = self.signing_gossip_engine.clone();

		let (misbehaviour_tx, mut misbehaviour_rx) = tokio::sync::mpsc::unbounded_channel();
		self.misbehaviour_tx = Some(misbehaviour_tx);
		let mut lock = self.error_handler_rx.write();
		let mut error_handler_rx = lock.take().expect("Error handler is only taken once; qed");
		drop(lock);
		self.initialization().await;
		log::debug!(target: "dkg_gadget::worker", "Starting DKG Iteration loop");
		// create a stream from each gossip engine queue.
		let mut keygen_stream = keygen_gossip_engine
			.message_available_notification()
			.filter_map(|_| futures::future::ready(keygen_gossip_engine.peek_last_message()));
		let mut signing_stream = signing_gossip_engine
			.message_available_notification()
			.filter_map(|_| futures::future::ready(signing_gossip_engine.peek_last_message()));

		loop {
			tokio::select! {
				// This will make the select a bit biased towards the keygen stream and signing
				// stream.
				biased;
				keygen_msg = keygen_stream.next().fuse() => {
					if let Some(msg) = keygen_msg {
						log::debug!(target: "dkg_gadget::worker", "Going to handle keygen message for round {}", msg.msg.round_id);
						match self.process_incoming_dkg_message(msg) {
							Ok(_) => {
								self.keygen_gossip_engine.acknowledge_last_message();
							},
							Err(e) => {
								log::error!(target: "dkg_gadget::worker", "Error processing keygen message: {:?}", e);
							},
						}
					}
				},
				signing_msg = signing_stream.next().fuse() => {
					if let Some(msg) = signing_msg {
						log::debug!(target: "dkg_gadget::worker", "Going to handle signing message for round {}", msg.msg.round_id);
						match self.process_incoming_dkg_message(msg) {
							Ok(_) => {
								self.signing_gossip_engine.acknowledge_last_message();
							},
							Err(e) => {
								log::error!(target: "dkg_gadget::worker", "Error processing signing message: {:?}", e);
							},
						}
					}
				},
				notification = self.import_notifications.next().fuse() => {
					if let Some(notification) = notification {
						log::debug!(target: "dkg_gadget::worker", "Going to handle Import notification");
						self.handle_import_notification(notification);
					} else {
						log::error!(target: "dkg_gadget::worker", "Import notification stream closed");
						break;
					}
				},
				notification = self.finality_notifications.next().fuse() => {
					if let Some(notification) = notification {
						log::debug!(target: "dkg_gadget::worker", "Going to handle Finality notification");
						self.handle_finality_notification(notification);
					} else {
						log::error!(target: "dkg_gadget::worker", "Finality notification stream closed");
						break;
					}
				},
				misbehaviour = misbehaviour_rx.recv().fuse() => {
					if let Some(misbehaviour) = misbehaviour {
						log::debug!(target: "dkg_gadget::worker", "Going to handle Misbehaviour");
						gossip_misbehaviour_report(&mut self, misbehaviour);
					}
				},
				error = error_handler_rx.recv().fuse() => {
					if let Some(error) = error {
						log::debug!(target: "dkg_gadget::worker", "Going to handle Error");
						self.handle_dkg_error(error);
					}
				},
			}
		}

		log::info!(target: "dkg_gadget::worker", "DKG Iteration loop finished");
	}
}

/// Extension trait for any type that contains a keystore
#[auto_impl::auto_impl(&mut, &, Arc)]
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

impl KeystoreExt for DKGKeystore {
	fn get_keystore(&self) -> &DKGKeystore {
		self
	}
}

#[auto_impl::auto_impl(&mut, &, Arc)]
pub trait HasLatestHeader<B: Block> {
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>>;
	/// Gets latest block number from latest block header
	fn get_latest_block_number(&self) -> NumberFor<B> {
		if let Some(latest_header) = self.get_latest_header().read().clone() {
			*latest_header.number()
		} else {
			NumberFor::<B>::from(0u32)
		}
	}
}

impl<B, BE, C, GE> HasLatestHeader<B> for DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
{
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}
}
