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
use curv::elliptic::curves::Secp256k1;
use dkg_logging::{debug, error, info, trace};
use dkg_primitives::utils::select_random_set;
use dkg_runtime_primitives::KEYGEN_TIMEOUT;
use futures::StreamExt;
use itertools::Itertools;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use sc_keystore::LocalKeystore;
use sp_core::ecdsa;
use std::{
	collections::{BTreeSet, HashMap, HashSet},
	future::Future,
	marker::PhantomData,
	pin::Pin,
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	},
};

use parking_lot::RwLock;

use sc_client_api::{Backend, FinalityNotification};

use sp_api::BlockId;
use sp_arithmetic::traits::CheckedRem;
use sp_runtime::traits::{Block, Header, NumberFor, Zero};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use dkg_primitives::{
	types::{
		DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, DKGMsgStatus, SessionId,
		SignedDKGMessage,
	},
	AuthoritySetId, DKGReport, MisbehaviourType, GOSSIP_MESSAGE_RESENDING_LIMIT,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	utils::to_slice_33,
	AggregatedMisbehaviourReports, AggregatedPublicKeys, AuthoritySet, DKGApi, UnsignedProposal,
	GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	async_protocols::{remote::AsyncProtocolRemote, AsyncProtocolParameters, GenericAsyncHandler},
	error,
	gossip_engine::GossipEngineIface,
	gossip_messages::{
		misbehaviour_report::{gossip_misbehaviour_report, handle_misbehaviour_report},
		public_key_gossip::handle_public_key_broadcast,
	},
	keystore::DKGKeystore,
	metric_inc, metric_set,
	metrics::Metrics,
	utils::find_authorities_change,
	Client,
};

pub const ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

pub const STORAGE_SET_RETRY_NUM: usize = 5;

pub const MAX_SUBMISSION_DELAY: u32 = 3;

pub const MAX_SIGNING_SETS: u64 = 32;

pub const MAX_KEYGEN_RETRIES: usize = 5;

pub const SESSION_PROGRESS_THRESHOLD: sp_runtime::Permill = sp_runtime::Permill::from_percent(100);

/// How many blocks to keep the proposal hash in out local cache.
pub const PROPOSAL_HASH_LIFETIME: u32 = 10;

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
	pub db_backend: Arc<dyn crate::db::DKGDbBackend>,
	pub metrics: Option<Metrics>,
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
	pub db: Arc<dyn crate::db::DKGDbBackend>,
	pub metrics: Arc<Option<Metrics>>,
	// Genesis keygen and rotated round
	pub rounds: Shared<Option<AsyncProtocolRemote<NumberFor<B>>>>,
	// Next keygen round, always taken and restarted each session
	pub next_rounds: Shared<Option<AsyncProtocolRemote<NumberFor<B>>>>,
	// Signing rounds, created everytime there are unique unsigned proposals
	pub signing_rounds: Shared<Vec<Option<AsyncProtocolRemote<NumberFor<B>>>>>,
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
	pub aggregated_public_keys: Shared<HashMap<SessionId, AggregatedPublicKeys>>,
	/// Tracking for the misbehaviour reports
	pub aggregated_misbehaviour_reports: Shared<AggregatedMisbehaviourReportStore>,
	pub misbehaviour_tx: Option<UnboundedSender<DKGMisbehaviourMessage>>,
	/// A HashSet of the currently being signed proposals.
	/// Note: we only store the hash of the proposal here, not the full proposal.
	pub currently_signing_proposals: Shared<HashSet<[u8; 32]>>,
	/// Tracking for sent gossip messages: using blake2_128 for message hashes
	/// The value is the number of times the message has been sent.
	pub has_sent_gossip_msg: Shared<HashMap<[u8; 16], u8>>,
	/// Concrete type that points to the actual local keystore if it exists
	pub local_keystore: Shared<Option<Arc<LocalKeystore>>>,
	/// For transmitting errors from parallel threads to the DKGWorker
	pub error_handler: tokio::sync::broadcast::Sender<DKGError>,
	/// Keep track of the number of how many times we have tried the keygen protocol.
	pub keygen_retry_count: Arc<AtomicUsize>,
	// keep rustc happy
	_backend: PhantomData<BE>,
}

// Implementing Clone for DKGWorker is required for the async protocol
impl<B, BE, C, GE> Clone for DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	fn clone(&self) -> Self {
		Self {
			client: self.client.clone(),
			backend: self.backend.clone(),
			key_store: self.key_store.clone(),
			db: self.db.clone(),
			keygen_gossip_engine: self.keygen_gossip_engine.clone(),
			signing_gossip_engine: self.signing_gossip_engine.clone(),
			metrics: self.metrics.clone(),
			rounds: self.rounds.clone(),
			next_rounds: self.next_rounds.clone(),
			signing_rounds: self.signing_rounds.clone(),
			best_authorities: self.best_authorities.clone(),
			best_next_authorities: self.best_next_authorities.clone(),
			latest_header: self.latest_header.clone(),
			current_validator_set: self.current_validator_set.clone(),
			queued_validator_set: self.queued_validator_set.clone(),
			aggregated_public_keys: self.aggregated_public_keys.clone(),
			aggregated_misbehaviour_reports: self.aggregated_misbehaviour_reports.clone(),
			misbehaviour_tx: self.misbehaviour_tx.clone(),
			has_sent_gossip_msg: self.has_sent_gossip_msg.clone(),
			currently_signing_proposals: self.currently_signing_proposals.clone(),
			local_keystore: self.local_keystore.clone(),
			error_handler: self.error_handler.clone(),
			keygen_retry_count: self.keygen_retry_count.clone(),
			_backend: PhantomData,
		}
	}
}

pub type AggregatedMisbehaviourReportStore =
	HashMap<(MisbehaviourType, SessionId, AuthorityId), AggregatedMisbehaviourReports<AuthorityId>>;

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
			db_backend,
			keygen_gossip_engine,
			signing_gossip_engine,
			metrics,
			local_keystore,
			latest_header,
			..
		} = worker_params;

		let (error_handler, _) = tokio::sync::broadcast::channel(1024);

		DKGWorker {
			client,
			misbehaviour_tx: None,
			backend,
			key_store,
			db: db_backend,
			keygen_gossip_engine: Arc::new(keygen_gossip_engine),
			signing_gossip_engine: Arc::new(signing_gossip_engine),
			metrics: Arc::new(metrics),
			rounds: Arc::new(RwLock::new(None)),
			next_rounds: Arc::new(RwLock::new(None)),
			signing_rounds: Arc::new(RwLock::new(vec![None; MAX_SIGNING_SETS as _])),
			best_authorities: Arc::new(RwLock::new(vec![])),
			best_next_authorities: Arc::new(RwLock::new(vec![])),
			current_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			queued_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			latest_header,
			aggregated_public_keys: Arc::new(RwLock::new(HashMap::new())),
			aggregated_misbehaviour_reports: Arc::new(RwLock::new(HashMap::new())),
			has_sent_gossip_msg: Arc::new(RwLock::new(HashMap::new())),
			currently_signing_proposals: Arc::new(RwLock::new(HashSet::new())),
			local_keystore: Arc::new(RwLock::new(local_keystore)),
			error_handler,
			keygen_retry_count: Arc::new(AtomicUsize::new(0)),
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
		session_id: SessionId,
		stage: ProtoStageType,
		async_index: u8,
		protocol_name: &str,
	) -> Result<AsyncProtocolParameters<DKGProtocolEngine<B, BE, C, GE>>, DKGError> {
		let best_authorities = Arc::new(best_authorities);
		let authority_public_key = Arc::new(authority_public_key);

		let now = self.get_latest_block_number();
		let status_handle = AsyncProtocolRemote::new(now, session_id);
		// Fetch the active key. This requires rotating the key to have happened with
		// full certainty in order to ensure the right key is being used to make signatures.
		let active_local_key = match stage {
			ProtoStageType::Genesis => None,
			ProtoStageType::Queued => None,
			ProtoStageType::Signing => {
				let optional_session_id = Some(session_id);
				let (active_local_key, _) = self.fetch_local_keys(optional_session_id);
				active_local_key
			},
		};
		let params = AsyncProtocolParameters {
			engine: Arc::new(DKGProtocolEngine {
				backend: self.backend.clone(),
				latest_header: self.latest_header.clone(),
				client: self.client.clone(),
				keystore: self.key_store.clone(),
				db: self.db.clone(),
				gossip_engine: self.get_gossip_engine_from_protocol_name(protocol_name),
				aggregated_public_keys: self.aggregated_public_keys.clone(),
				best_authorities: best_authorities.clone(),
				authority_public_key: authority_public_key.clone(),
				current_validator_set: self.current_validator_set.clone(),
				local_keystore: self.local_keystore.clone(),
				vote_results: Arc::new(Default::default()),
				is_genesis: stage == ProtoStageType::Genesis,
				metrics: self.metrics.clone(),
				_pd: Default::default(),
			}),
			session_id,
			db: self.db.clone(),
			keystore: self.key_store.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities,
			authority_public_key,
			batch_id_gen: Arc::new(Default::default()),
			handle: status_handle.clone(),
			local_key: active_local_key,
		};
		// Start the respective protocol
		status_handle.start()?;
		// Cache the rounds, respectively
		match stage {
			ProtoStageType::Genesis => {
				debug!(target: "dkg_gadget::worker", "Starting genesis protocol (obtaing the lock)");
				let mut lock = self.rounds.write();
				debug!(target: "dkg_gadget::worker", "Starting genesis protocol (got the lock)");
				if lock.is_some() {
					dkg_logging::warn!(target: "dkg_gadget::worker", "Overwriting rounds will result in termination of previous rounds!");
				}
				*lock = Some(status_handle);
			},
			ProtoStageType::Queued => {
				debug!(target: "dkg_gadget::worker", "Starting queued protocol (obtaing the lock)");
				let mut lock = self.next_rounds.write();
				debug!(target: "dkg_gadget::worker", "Starting queued protocol (got the lock)");
				if lock.is_some() {
					dkg_logging::warn!(target: "dkg_gadget::worker", "Overwriting rounds will result in termination of previous rounds!");
				}
				*lock = Some(status_handle);
			},
			// When we are at signing stage, it is using the active rounds.
			ProtoStageType::Signing => {
				debug!(target: "dkg_gadget::worker", "Starting signing protocol: async_index #{}", async_index);
				let mut lock = self.signing_rounds.write();
				// first, check if the async_index is already in use and if so, and it is still
				// running, return an error and print a warning that we will overwrite the previous
				// round.
				if let Some(Some(current_round)) = lock.get(async_index as usize) {
					// check if it has stalled or not, if so, we can overwrite it
					// TODO: Write more on what we should be going here since it's all the same
					if current_round.signing_has_stalled(now) {
						// the round has stalled, so we can overwrite it
						dkg_logging::warn!(target: "dkg_gadget::worker", "signing round async index #{} has stalled, overwriting it", async_index);
						lock[async_index as usize] = Some(status_handle)
					} else if current_round.is_active() {
						dkg_logging::warn!(target: "dkg_gadget::worker", "Overwriting rounds will result in termination of previous rounds!");
						lock[async_index as usize] = Some(status_handle)
					} else {
						// the round is not active, nor has it stalled, so we can overwrite it.
						dkg_logging::debug!(target: "dkg_gadget::worker", "signing round async index #{} is not active, overwriting it", async_index);
						lock[async_index as usize] = Some(status_handle)
					}
				} else {
					// otherwise, we can safely write to this slot.
					lock[async_index as usize] = Some(status_handle);
				}
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
		&self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
	) {
		match self.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			session_id,
			stage,
			0u8,
			crate::DKG_KEYGEN_PROTOCOL_NAME,
		) {
			Ok(async_proto_params) => {
				let err_handler_tx = self.error_handler.clone();
				// Check first from the rounds object, if any.
				let status = if let Some(rounds) = self.rounds.read().as_ref() {
					if rounds.session_id == session_id {
						DKGMsgStatus::ACTIVE
					} else {
						DKGMsgStatus::QUEUED
					}
				} else if session_id == GENESIS_AUTHORITY_SET_ID {
					// We are likely crashed and restarted, so we do not have the rounds object,
					// yet. We can safely assume that we are in the genesis stage since we are
					// session 0.
					DKGMsgStatus::ACTIVE
				} else {
					// We are likely crashed and restarted, and we are not in the genesis stage,
					// so we can safely assume that we are in the queued state.
					DKGMsgStatus::QUEUED
				};
				match GenericAsyncHandler::setup_keygen(async_proto_params, threshold, status) {
					Ok(meta_handler) => {
						let task = async move {
							match meta_handler.await {
								Ok(_) => {
									info!(target: "dkg_gadget::worker", "The meta handler has executed successfully");
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
	#[cfg_attr(
		feature = "tracing",
		dkg_logging::instrument(
			target = "dkg",
			skip_all,
			err,
			fields(session_id, threshold, stage, async_index)
		)
	)]
	fn create_signing_protocol(
		&self,
		best_authorities: Vec<Public>,
		authority_public_key: Public,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
		unsigned_proposals: Vec<UnsignedProposal>,
		signing_set: Vec<u16>,
		async_index: u8,
	) -> Result<Pin<Box<dyn Future<Output = Result<u8, DKGError>> + Send + 'static>>, DKGError> {
		let async_proto_params = self.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			session_id,
			stage,
			async_index,
			crate::DKG_SIGNING_PROTOCOL_NAME,
		)?;

		let err_handler_tx = self.error_handler.clone();
		let proposal_hashes =
			unsigned_proposals.iter().filter_map(|p| p.hash()).collect::<Vec<_>>();
		let meta_handler = GenericAsyncHandler::setup_signing(
			async_proto_params,
			threshold,
			unsigned_proposals,
			signing_set,
			async_index,
		)?;
		let currently_signing_proposals = self.currently_signing_proposals.clone();
		let task = async move {
			match meta_handler.await {
				Ok(_) => {
					info!(target: "dkg_gadget::worker", "The meta handler has executed successfully");
					Ok(async_index)
				},

				Err(err) => {
					error!(target: "dkg_gadget::worker", "Error executing meta handler {:?}", &err);
					let _ = err_handler_tx.send(err.clone());
					// remove proposal hashes, so that they can be reprocessed
					let mut lock = currently_signing_proposals.write();
					proposal_hashes.iter().for_each(|h| {
						lock.remove(h);
					});
					Err(err)
				},
			}
		};

		Ok(Box::pin(task))
	}

	/// Fetch the stored local keys if they exist.
	///
	/// The `optional_session_id` is used to fetch the keys for a specific session, only in case
	/// if `self.rounds` is `None`. This is useful when the node is restarted and we need to fetch
	/// the keys for the current session.
	fn fetch_local_keys(
		&self,
		optional_session_id: Option<SessionId>,
	) -> (Option<LocalKey<Secp256k1>>, Option<LocalKey<Secp256k1>>) {
		let current_session_id =
			self.rounds.read().as_ref().map(|r| r.session_id).or(optional_session_id);
		debug!(target: "dkg_gadget::worker", "🕸️  Fetching local keys for session id : {:?}", current_session_id);
		let next_session_id = current_session_id.map(|s| s + 1);
		let active_local_key =
			current_session_id.and_then(|s| self.db.get_local_key(s).ok().flatten());
		let next_local_key = next_session_id.and_then(|s| self.db.get_local_key(s).ok().flatten());
		debug!(target: "dkg_gadget::worker", "🕸️  Fetched active-local-key : {:?}", active_local_key);
		debug!(target: "dkg_gadget::worker", "🕸️  Fetched next-local-key : {:?}", next_local_key);
		(active_local_key, next_local_key)
	}

	/// Get the party index of our worker
	///
	/// Returns `None` if we are not in the best authority set
	pub fn get_party_index(&self, header: &B::Header) -> Option<u16> {
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
	pub fn get_next_party_index(&self, header: &B::Header) -> Option<u16> {
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

	/// Returns the progress of current session
	pub fn get_current_session_progress(&self, header: &B::Header) -> Option<sp_runtime::Permill> {
		let at: BlockId<B> = BlockId::hash(header.hash());
		return self
			.client
			.runtime_api()
			.get_current_session_progress(&at, *header.number())
			.unwrap_or_default()
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
		&self,
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
			// For when we already completed the DKG, no need to do it again.
			Some(rounds) if rounds.is_completed() => {
				debug!(target: "dkg_gadget::worker", "🕸️  Rounds exists and is completed");
				return
			},
			_ => {},
		}

		// DKG keygen authorities are always taken from the best set of authorities
		let session_id = genesis_authority_set.id;
		let maybe_party_index = self.get_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST GENESIS AUTHORITIES: session: {session_id:?}");
			*self.rounds.write() = None;
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST GENESIS AUTHORITIES: session: {session_id:?}, party_index: {maybe_party_index:?}");
		}

		let best_authorities: Vec<Public> =
			self.get_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_signature_threshold(header);
		let authority_public_key = self.get_authority_public_key();
		self.spawn_keygen_protocol(
			best_authorities,
			authority_public_key,
			session_id,
			threshold,
			ProtoStageType::Genesis,
		);
	}

	fn handle_queued_dkg_setup(&self, header: &B::Header, queued: AuthoritySet<Public>) {
		// Check if the authority set is empty, return or proceed
		if queued.authorities.is_empty() {
			debug!(target: "dkg_gadget::worker", "🕸️  queued authority set is empty");
			return
		}
		// Handling edge cases when the rounds exists, is currently active, and not stalled
		if let Some(rounds) = self.next_rounds.read().as_ref() {
			// Check if the next rounds exists and has processed for this next queued round id
			if rounds.is_active() && !rounds.keygen_has_stalled(*header.number()) {
				debug!(target: "dkg_gadget::worker", "🕸️  Next rounds exists and is active, returning...");
				return
			} else {
				// Proceed to clear the next rounds.
				debug!(target: "dkg_gadget::worker", "🕸️  Next rounds keygen has stalled, creating new rounds...");
			}
		}
		// Get the best next authorities using the keygen threshold
		let session_id = queued.id;
		let maybe_party_index = self.get_next_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST NEXT AUTHORITIES: session {:?}", session_id);
			*self.next_rounds.write() = None;
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST NEXT AUTHORITIES: session {:?}", session_id);
		}

		*self.best_next_authorities.write() = self.get_next_best_authorities(header);
		let next_best_authorities: Vec<Public> =
			self.get_next_best_authorities(header).iter().map(|x| x.1.clone()).collect();
		let threshold = self.get_next_signature_threshold(header);

		let authority_public_key = self.get_authority_public_key();
		// spawn the Keygen protocol for the Queued DKG.
		debug!(target: "dkg_gadget::worker", "🕸️  Spawning keygen protocol for queued DKG");
		self.spawn_keygen_protocol(
			next_best_authorities,
			authority_public_key,
			session_id,
			threshold,
			ProtoStageType::Queued,
		);
	}

	// *** Block notifications ***
	fn process_block_notification(&self, header: &B::Header) {
		if let Some(latest_header) = self.latest_header.read().clone() {
			if latest_header.number() >= header.number() {
				// We've already seen this block, ignore it.
				debug!(target: "dkg_gadget::worker", "🕸️  Latest header is greater than or equal to current header, returning...");
				return
			}
		}
		debug!(target: "dkg_gadget::worker", "🕸️  Processing block notification for block {}", header.number());
		metric_set!(self, dkg_latest_block_height, header.number());
		*self.latest_header.write() = Some(header.clone());
		debug!(target: "dkg_gadget::worker", "🕸️  Latest header is now: {:?}", header.number());
		// Check if we should execute emergency Keygen Protocol.
		if self.should_execute_emergency_keygen(header) {
			debug!(target: "dkg_gadget::worker", "🕸️  Should execute emergency keygen");
			self.handle_emergency_keygen(header);
			return
		}
		// Attempt to enact new DKG authorities if sessions have changed

		// The Steps for enacting new DKG authorities are:
		// 1. Check if the DKG Public Key are not yet set on chain (or not yet generated)
		// 2. if yes, we start enacting authorities on genesis flow.
		// 3. if no, we start enacting authorities on queued flow and submit any unsigned
		//          proposals.
		if self.get_dkg_pub_key(header).1.is_empty() {
			self.maybe_enact_genesis_authorities(header);
		} else {
			self.maybe_enact_next_authorities(header);
			self.maybe_rotate_local_sessions(header);
			self.submit_unsigned_proposals(header);
		}
	}

	fn maybe_enact_genesis_authorities(&self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, _queued)) = self.validator_set(header) {
			// If we are in the genesis state, we need to enact the genesis authorities
			if active.id == GENESIS_AUTHORITY_SET_ID {
				debug!(target: "dkg_gadget::worker", "🕸️  GENESIS SESSION ID {:?}", active.id);
				metric_set!(self, dkg_validator_set_id, active.id);
				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				// Setting new validator set id as current
				*self.current_validator_set.write() = active.clone();
				*self.best_authorities.write() = self.get_best_authorities(header);
				*self.best_next_authorities.write() = self.get_next_best_authorities(header);
				// Setting up the DKG
				self.handle_genesis_dkg_setup(header, active);
			}
		}
	}

	/// Should enact next authorities will check for the follwoing situations:
	///
	/// If the session period has not elapsed, we will return early.
	///
	/// 1. If we detect a change in the best authorities, we should enact the next authorities with
	/// the new ones.
	/// 2. If the session progress is greater than the threshold, we should enact the next
	/// authorities.
	///
	/// Edge cases:
	/// 1. If we already running a keygen protocol, and we detected that we are stalled, this
	///    method will try to restart the keygen protocol.
	fn maybe_enact_next_authorities(&self, header: &B::Header) {
		// Query the current state of session progress, we will proceed with enact next
		// authorities if the session progress has passed threshold
		if let Some(session_progress) = self.get_current_session_progress(header) {
			debug!(target: "dkg_gadget::worker", "🕸️  Session progress percentage : {:?}", session_progress);
			metric_set!(self, dkg_session_progress, session_progress.deconstruct());
			if session_progress < SESSION_PROGRESS_THRESHOLD {
				debug!(target: "dkg_gadget::worker", "🕸️  Session progress percentage below threshold!");
				return
			}
		} else {
			debug!(target: "dkg_gadget::worker", "🕸️  Unable to retrive session progress percentage!");
			return
		}
		// Get the active and queued validators to check for updates
		if let Some((_active, queued)) = self.validator_set(header) {
			debug!(target: "dkg_gadget::worker", "🕸️  Session progress percentage above threshold, proceed with enact new authorities");
			// Check if there is a keygen is finished:
			let queued_keygen_finished = self
				.next_rounds
				.read()
				.as_ref()
				.map(|r| r.is_keygen_finished())
				.unwrap_or(false);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED KEYGEN FINISHED: {:?}", queued_keygen_finished);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED DKG STATUS: {:?}", self.next_rounds.read().as_ref().map(|r| r.status.clone()));
			if queued_keygen_finished {
				return
			}

			let has_next_rounds = self.next_rounds.read().is_some();
			debug!(target: "dkg_gadget::worker", "🕸️  HAS NEXT ROUND KEYGEN: {:?}", has_next_rounds);
			// Check if there is a next DKG Key on-chain.
			let next_dkg_key = self.get_next_dkg_pub_key(header);
			debug!(target: "dkg_gadget::worker", "🕸️  NEXT DKG KEY ON CHAIN: {}", next_dkg_key.is_some());
			// Start a keygen if we don't have one.
			// only if there is no queued key on chain.
			if !has_next_rounds && next_dkg_key.is_none() {
				// Start the queued DKG setup for the new queued authorities
				self.handle_queued_dkg_setup(header, queued);
				// Reset the Retry counter.
				self.keygen_retry_count.store(0, Ordering::SeqCst);
				return
			}

			// Check if we are stalled:
			// a read only clone, to avoid holding the lock for the whole duration of the function
			let lock = self.next_rounds.read();
			let next_rounds_clone = (*lock).clone();
			drop(lock);
			if let Some(ref rounds) = next_rounds_clone {
				debug!(target: "dkg_gadget::worker", "🕸️  Status: {:?}, Now: {:?}, Started At: {:?}, Timeout length: {:?}", rounds.status, header.number(), rounds.started_at, KEYGEN_TIMEOUT);
				let keygen_stalled = rounds.keygen_has_stalled(*header.number());
				let (current_attmp, max, should_retry) = {
					// check how many authorities are in the next best authorities
					// and then check the signature threshold `t`, if `t+1` is greater than the
					// number of authorities and we still have not reached the maximum number of
					// retries, we should retry the keygen
					let next_best = self.get_next_best_authorities(header);
					let n = next_best.len();
					let t = self.get_next_signature_threshold(header) as usize;
					// in this case, if t + 1 is equal to n, we should retry the keygen
					// indefinitely.
					// For example, if we are running a 3 node network, with 1-of-2 DKG, it will not
					// be possible to successfully report the DKG Misbehavior on chain.
					let max_retries = if t + 1 == n { 0 } else { MAX_KEYGEN_RETRIES };
					let v = self.keygen_retry_count.load(Ordering::SeqCst);
					let should_retry = v < max_retries || max_retries == 0;
					if keygen_stalled {
						debug!(
							target: "dkg_gadget::worker",
							"Calculated retry conditions => n: {n}, t: {t}, current_attempt: {v}, max: {max_retries}, should_retry: {should_retry}"
						);
					}
					(v, max_retries, should_retry)
				};
				if keygen_stalled && should_retry {
					debug!(target: "dkg_gadget::worker", "🕸️  Queued Keygen has stalled, retrying (attempt: {}/{})", current_attmp, max);
					metric_inc!(self, dkg_keygen_retry_counter);
					// Start the queued Keygen protocol again.
					self.handle_queued_dkg_setup(header, queued);
					// Increment the retry count
					self.keygen_retry_count.fetch_add(1, Ordering::SeqCst);
				} else if keygen_stalled && !should_retry {
					debug!(target: "dkg_gadget::worker", "🕸️  Queued Keygen has stalled, but we have reached the maximum number of retries will report bad actors.");
					self.handle_dkg_error(DKGError::KeygenTimeout {
						bad_actors: convert_u16_vec_to_usize_vec(
							rounds.current_round_blame().blamed_parties,
						),
						session_id: rounds.session_id,
					})
				}
			}
		}
	}

	fn maybe_rotate_local_sessions(&self, header: &B::Header) {
		if let Some((active, queued)) = self.validator_set(header) {
			debug!(target: "dkg_gadget::worker", "🕸️  ACTIVE SESSION ID {:?}", active.id);
			metric_set!(self, dkg_validator_set_id, active.id);
			// verify the new validator set
			let _ = self.verify_validator_set(header.number(), active.clone());
			// Check if the on chain authority_set_id is the same as the queued_authority_set_id.
			let (set_id, _) = self.get_dkg_pub_key(header);
			let queued_authority_set_id = self.queued_validator_set.read().id;
			debug!(target: "dkg_gadget::worker", "🕸️  CURRENT SET ID: {:?}", set_id);
			debug!(target: "dkg_gadget::worker", "🕸️  QUEUED AUTHORITY SET ID: {:?}", queued_authority_set_id);
			if set_id != queued_authority_set_id {
				return
			}
			// Update the validator sets
			*self.current_validator_set.write() = active;
			*self.queued_validator_set.write() = queued;
			dkg_logging::debug!(target: "dkg_gadget::worker", "🕸️  Rotating next round this will result in a drop/termination of the current rounds!");
			match self.rounds.read().as_ref() {
				Some(r) if r.is_active() => {
					dkg_logging::warn!(target: "dkg_gadget::worker", "🕸️  Current rounds is active, rotating next round will terminate it!!");
				},
				Some(_) | None => {
					dkg_logging::warn!(target: "dkg_gadget::worker", "🕸️  Current rounds is not active, rotating next rounds is okay");
				},
			};
			*self.rounds.write() = self.next_rounds.write().take();
			// We also rotate the best authority caches
			*self.best_authorities.write() = self.best_next_authorities.read().clone();
			*self.best_next_authorities.write() = self.get_next_best_authorities(header);
			// since we just rotate, we reset the keygen retry counter
			self.keygen_retry_count.store(0, Ordering::Relaxed);
			// clear the currently being signing proposals cache.
			self.currently_signing_proposals.write().clear();
			// Reset all the signing rounds.
			self.signing_rounds.write().iter_mut().for_each(|v| {
				if let Some(r) = v.as_mut() {
					let _ = r.shutdown("Rotating next round");
				}
				*v = None;
			});
			// Reset per session metrics
			if let Some(metrics) = self.metrics.as_ref() {
				metrics.reset_session_metrics();
			}
		}
	}

	fn handle_finality_notification(&self, notification: FinalityNotification<B>) {
		trace!(target: "dkg_gadget::worker", "🕸️  Finality notification: {:?}", notification);
		// Handle finality notifications
		self.process_block_notification(&notification.header);
	}

	#[cfg_attr(
		feature = "tracing",
		dkg_logging::instrument(target = "dkg", skip_all, ret, err, fields(signed_dkg_message))
	)]
	fn verify_signature_against_authorities(
		&self,
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

	pub fn handle_emergency_keygen(&self, header: &B::Header) {
		// Start the queued DKG setup for the new queued authorities
		if let Some((_active, queued)) = self.validator_set(header) {
			self.handle_queued_dkg_setup(header, queued);
		}
	}

	#[cfg_attr(
		feature = "tracing",
		dkg_logging::instrument(target = "dkg", skip_all, fields(dkg_error))
	)]
	pub fn handle_dkg_error(&self, dkg_error: DKGError) {
		dkg_logging::error!(target: "dkg_gadget::worker", "Received error: {:?}", dkg_error);
		metric_inc!(self, dkg_error_counter);
		let authorities: Vec<Public> =
			self.best_authorities.read().iter().map(|x| x.1.clone()).collect();

		let (bad_actors, session_id) = match dkg_error {
			DKGError::KeygenMisbehaviour { ref bad_actors, .. } => {
				metric_inc!(self, dkg_keygen_misbehaviour_error);
				(bad_actors.clone(), 0)
			},
			DKGError::KeygenTimeout { ref bad_actors, session_id, .. } => {
				metric_inc!(self, dkg_keygen_timeout_error);
				(bad_actors.clone(), session_id)
			},
			// Todo: Handle Signing Timeout as a separate case
			DKGError::SignMisbehaviour { ref bad_actors, .. } => {
				metric_inc!(self, dkg_sign_misbehaviour_error);
				(bad_actors.clone(), 0)
			},
			_ => Default::default(),
		};

		dkg_logging::error!(target: "dkg_gadget::worker", "Bad Actors : {:?}, Session Id : {:?}", bad_actors, session_id);

		let mut offenders: Vec<AuthorityId> = Vec::new();
		for bad_actor in bad_actors {
			let bad_actor = bad_actor;
			if bad_actor > 0 && bad_actor <= authorities.len() {
				if let Some(offender) = authorities.get(bad_actor - 1) {
					offenders.push(offender.clone());
				}
			}
		}

		for offender in offenders {
			match dkg_error {
				DKGError::KeygenMisbehaviour { bad_actors: _, .. } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender, session_id }),
				DKGError::KeygenTimeout { .. } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender, session_id }),
				DKGError::SignMisbehaviour { bad_actors: _, .. } =>
					self.handle_dkg_report(DKGReport::SignMisbehaviour { offender, session_id }),
				_ => (),
			}
		}
	}

	/// Route messages internally where they need to be routed
	#[cfg_attr(
		feature = "tracing",
		dkg_logging::instrument(target = "dkg", skip_all, ret, err, fields(dkg_msg))
	)]
	fn process_incoming_dkg_message(
		&self,
		dkg_msg: SignedDKGMessage<Public>,
	) -> Result<(), DKGError> {
		metric_inc!(self, dkg_inbound_messages);
		// discard the message if from previous round
		if let Some(current_round) = self.rounds.read().as_ref() {
			if dkg_msg.msg.session_id < current_round.session_id {
				dkg_logging::warn!(target: "dkg_gadget::worker", "Message is for already completed round: {}, Discarding message", dkg_msg.msg.session_id);
				return Ok(())
			}
		}

		match &dkg_msg.msg.payload {
			DKGMsgPayload::Keygen(_) => {
				let msg = Arc::new(dkg_msg);
				if let Some(rounds) = self.rounds.read().as_ref() {
					if rounds.session_id == msg.msg.session_id {
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
						return Ok(())
					}
				}

				if let Some(rounds) = self.next_rounds.read().as_ref() {
					if rounds.session_id == msg.msg.session_id {
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
						return Ok(())
					}
				}

				Ok(())
			},
			DKGMsgPayload::Offline(..) | DKGMsgPayload::Vote(..) => {
				let msg = Arc::new(dkg_msg);
				let async_index = msg.msg.payload.get_async_index();
				dkg_logging::debug!(target: "dkg_gadget::worker", "Received message for async index {}", async_index);
				if let Some(Some(rounds)) = self.signing_rounds.read().get(async_index as usize) {
					dkg_logging::debug!(target: "dkg_gadget::worker", "Message is for signing execution in session {}", rounds.session_id);
					if rounds.session_id == msg.msg.session_id {
						dkg_logging::debug!(target: "dkg_gadget::worker", "Message is for this signing execution in session: {}", rounds.session_id);
						if let Err(err) = rounds.deliver_message(msg) {
							self.handle_dkg_error(DKGError::CriticalError {
								reason: err.to_string(),
							})
						}
					} else {
						dkg_logging::warn!(target: "dkg_gadget::worker", "Message is for another signing round: {}", rounds.session_id);
					}
				} else {
					dkg_logging::warn!(target: "dkg_gadget::worker", "No signing rounds for async index {}", async_index);
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
						dkg_logging::error!(target: "dkg_gadget::worker", "Error while verifying signature against authorities: {:?}", err)
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
						dkg_logging::error!(target: "dkg_gadget::worker", "Error while verifying signature against authorities: {:?}", err)
					},
				}

				Ok(())
			},
		}
	}

	fn handle_dkg_report(&self, dkg_report: DKGReport) {
		let (offender, session_id, misbehaviour_type) = match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender, session_id } => {
				info!(target: "dkg_gadget::worker", "🕸️  DKG Keygen misbehaviour @ Session ({session_id}) by {offender}");
				(offender, session_id, MisbehaviourType::Keygen)
			},
			DKGReport::SignMisbehaviour { offender, session_id } => {
				info!(target: "dkg_gadget::worker", "🕸️  DKG Signing misbehaviour @ Session ({session_id}) by {offender}");
				(offender, session_id, MisbehaviourType::Sign)
			},
		};

		let misbehaviour_msg =
			DKGMisbehaviourMessage { misbehaviour_type, session_id, offender, signature: vec![] };
		let hash = sp_core::blake2_128(&misbehaviour_msg.encode());
		let count = *self.has_sent_gossip_msg.read().get(&hash).unwrap_or(&0u8);
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

	fn submit_unsigned_proposals(&self, header: &B::Header) {
		let on_chain_dkg = self.get_dkg_pub_key(header);
		let session_id = on_chain_dkg.0;
		let dkg_pub_key = on_chain_dkg.1;
		let at: BlockId<B> = BlockId::hash(header.hash());
		let maybe_party_index = self.get_party_index(header);
		// Check whether the worker is in the best set or return
		if maybe_party_index.is_none() {
			info!(target: "dkg_gadget::worker", "🕸️  NOT IN THE SET OF BEST AUTHORITIES: session {:?}", session_id);
			return
		} else {
			info!(target: "dkg_gadget::worker", "🕸️  IN THE SET OF BEST AUTHORITIES: session {:?}", session_id);
		}

		// check if we should clear our proposal hash cache,
		// the condition is that `PROPOSAL_HASH_LIFETIME` blocks have passed since the last
		// block time we cached a proposal hash for.
		// this could be done without actually keeping track of the last block time we cached a
		// proposal hash for, by taking the modulo of the block number with
		// `PROPOSAL_HASH_LIFETIME`,
		let should_clear_proposals_cache = {
			// take the modulo of the block number with `PROPOSAL_HASH_LIFETIME`
			// if the result is 0, then `PROPOSAL_HASH_LIFETIME` blocks have passed since the last
			// block time we cached a proposal hash for.
			header
				.number()
				.checked_rem(&PROPOSAL_HASH_LIFETIME.into())
				.map(|x| x.is_zero())
				.unwrap_or(false)
		};

		if should_clear_proposals_cache {
			self.currently_signing_proposals.write().clear();
		}

		let unsigned_proposals = match self.client.runtime_api().get_unsigned_proposals(&at) {
			Ok(res) => {
				let mut filtered_unsigned_proposals = Vec::new();
				for proposal in res {
					if let Some(hash) = proposal.hash() {
						if !self.currently_signing_proposals.read().contains(&hash) {
							// update unsigned proposal counter
							metric_inc!(self, dkg_unsigned_proposal_counter);
							filtered_unsigned_proposals.push(proposal);
						}
					}
				}
				filtered_unsigned_proposals
			},
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
		let mut count = 0;
		let mut seed = dkg_pub_key;

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
		metric_set!(self, dkg_signing_sets, signing_sets.len());
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
				info!(target: "dkg_gadget::worker", "🕸️  Session Id {:?} | Async index {:?} | {}-out-of-{} signers: ({:?})", session_id, i, threshold, best_authorities.len(), signing_sets[i].clone());
				match self.create_signing_protocol(
					best_authorities.clone(),
					authority_public_key.clone(),
					session_id,
					threshold,
					ProtoStageType::Signing,
					unsigned_proposals.clone(),
					signing_sets[i].clone().into_iter().sorted().collect::<Vec<_>>(),
					// using i here as the async index is not correct at all,
					// instead we should find a free index in the `signing_rounds` and use that
					//
					// FIXME: use a free index in the `signing_rounds` instead of `i`
					i as _,
				) {
					Ok(task) => futures.push(task),
					Err(err) => {
						dkg_logging::error!(target: "dkg_gadget::worker", "Error creating signing protocol: {:?}", &err);
						self.handle_dkg_error(err)
					},
				}
			}
		}

		if futures.is_empty() {
			dkg_logging::error!(target: "dkg_gadget::worker", "While creating the signing protocol, 0 were created");
		} else {
			let proposal_hashes =
				unsigned_proposals.iter().filter_map(|x| x.hash()).collect::<Vec<_>>();
			// save the proposal hashes in the currently_signing_proposals.
			// this is used to check if we have already signed a proposal or not.
			self.currently_signing_proposals.write().extend(proposal_hashes);
			// the goal of the meta task is to select the first winner
			let meta_signing_protocol = async move {
				// select the first future to return Ok(()), ignoring every failure
				// (note: the errors are not truly ignored since each individual future
				// has logic to handle errors internally, including misbehaviour monitors
				let mut results = futures::future::select_ok(futures).await.into_iter();
				if let Some((_success, _losing_futures)) = results.next() {
					info!(target: "dkg_gadget::worker", "*** SUCCESSFULLY EXECUTED meta signing protocol {:?} ***", _success);
				} else {
					dkg_logging::warn!(target: "dkg_gadget::worker", "*** UNSUCCESSFULLY EXECUTED meta signing protocol");
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
			reason: format!("generate_signers failed, reason: {err}"),
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

	fn should_execute_emergency_keygen(&self, header: &B::Header) -> bool {
		// query runtime api to check if we should execute emergency keygen.
		let at: BlockId<B> = BlockId::hash(header.hash());
		self.client
			.runtime_api()
			.should_execute_emergency_keygen(&at)
			.unwrap_or_default()
	}

	/// Wait for initial finalized block
	async fn initialization(&mut self) {
		use futures::future;
		self.client
			.finality_notification_stream()
			.take_while(|notif| {
				if let Some((active, queued)) = self.validator_set(&notif.header) {
					// Cache the authority sets and best authorities
					*self.best_authorities.write() = self.get_best_authorities(&notif.header);
					*self.current_validator_set.write() = active;
					*self.queued_validator_set.write() = queued;
					// Route this to the finality notification handler
					self.handle_finality_notification(notif.clone());
					dkg_logging::debug!(target: "dkg_gadget::worker", "Initialization complete");
					// End the initialization stream
					future::ready(false)
				} else {
					future::ready(true)
				}
			})
			.for_each(|_| future::ready(()))
			.await;
	}

	// *** Main run loop ***
	pub(crate) async fn run(mut self) {
		let (misbehaviour_tx, misbehaviour_rx) = tokio::sync::mpsc::unbounded_channel();
		self.misbehaviour_tx = Some(misbehaviour_tx);
		self.initialization().await;
		dkg_logging::debug!(target: "dkg_gadget::worker", "Starting DKG Iteration loop");
		// We run all these tasks in parallel and wait for any of them to complete.
		// If any of them completes, we stop all the other tasks since this means a fatal error has
		// occurred and we need to shut down.
		let (first, n, ..) = futures::future::select_all(vec![
			self.spawn_finality_notification_task(),
			self.spawn_keygen_messages_stream_task(),
			self.spawn_signing_messages_stream_task(),
			self.spawn_error_handling_task(),
			self.spawn_misbehaviour_report_task(misbehaviour_rx),
		])
		.await;
		dkg_logging::error!(target: "dkg_gadget::worker", "DKG Worker finished; the reason that task({n}) ended with: {:?}", first);
	}

	fn spawn_finality_notification_task(&self) -> tokio::task::JoinHandle<()> {
		let mut stream = self.client.finality_notification_stream();
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(notification) = stream.next().await {
				dkg_logging::debug!(target: "dkg_gadget::worker", "Going to handle Finality notification");
				self_.handle_finality_notification(notification);
			}
		})
	}

	fn spawn_keygen_messages_stream_task(&self) -> tokio::task::JoinHandle<()> {
		let keygen_gossip_engine = self.keygen_gossip_engine.clone();
		let mut keygen_stream = keygen_gossip_engine
			.message_available_notification()
			.filter_map(move |_| futures::future::ready(keygen_gossip_engine.peek_last_message()));
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(msg) = keygen_stream.next().await {
				dkg_logging::debug!(target: "dkg_gadget::worker", "Going to handle keygen message for session {}", msg.msg.session_id);
				match self_.process_incoming_dkg_message(msg) {
					Ok(_) => {
						self_.keygen_gossip_engine.acknowledge_last_message();
					},
					Err(e) => {
						dkg_logging::error!(target: "dkg_gadget::worker", "Error processing keygen message: {:?}", e);
					},
				}
			}
		})
	}

	fn spawn_signing_messages_stream_task(&self) -> tokio::task::JoinHandle<()> {
		let signing_gossip_engine = self.signing_gossip_engine.clone();
		let mut signing_stream = signing_gossip_engine
			.message_available_notification()
			.filter_map(move |_| futures::future::ready(signing_gossip_engine.peek_last_message()));
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(msg) = signing_stream.next().await {
				dkg_logging::debug!(target: "dkg_gadget::worker", "Going to handle signing message for session {}", msg.msg.session_id);
				match self_.process_incoming_dkg_message(msg) {
					Ok(_) => {
						self_.signing_gossip_engine.acknowledge_last_message();
					},
					Err(e) => {
						dkg_logging::error!(target: "dkg_gadget::worker", "Error processing signing message: {:?}", e);
					},
				}
			}
		})
	}

	fn spawn_misbehaviour_report_task(
		&self,
		mut misbehaviour_rx: UnboundedReceiver<DKGMisbehaviourMessage>,
	) -> tokio::task::JoinHandle<()> {
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(misbehaviour) = misbehaviour_rx.recv().await {
				dkg_logging::debug!(target: "dkg_gadget::worker", "Going to handle Misbehaviour");
				gossip_misbehaviour_report(&self_, misbehaviour);
			}
		})
	}

	fn spawn_error_handling_task(&self) -> tokio::task::JoinHandle<()> {
		let self_ = self.clone();
		let mut error_handler_rx = self.error_handler.subscribe();
		tokio::spawn(async move {
			while let Ok(error) = error_handler_rx.recv().await {
				dkg_logging::debug!(target: "dkg_gadget::worker", "Going to handle Error");
				self_.handle_dkg_error(error);
			}
		})
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
