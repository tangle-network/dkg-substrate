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
	async_protocols::{blockchain_interface::DKGProtocolEngine, KeygenPartyId},
	debug_logger::DebugLogger,
};
use codec::{Codec, Encode};
use curv::elliptic::curves::Secp256k1;
use sc_network::NetworkService;
use sc_network_sync::SyncingService;
use sp_consensus::SyncOracle;

use crate::signing_manager::SigningManager;
use futures::StreamExt;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use parking_lot::{Mutex, RwLock};
use sc_client_api::{Backend, FinalityNotification};
use sc_keystore::LocalKeystore;
use sp_arithmetic::traits::SaturatedConversion;
use sp_core::ecdsa;
use sp_runtime::traits::{Block, Get, Header, NumberFor};
use std::{
	collections::{BTreeSet, HashMap},
	marker::PhantomData,
	pin::Pin,
	sync::{atomic::Ordering, Arc},
};
use tokio::sync::mpsc::UnboundedSender;

use dkg_primitives::{
	types::{DKGError, DKGMessage, NetworkMsgPayload, SessionId, SignedDKGMessage},
	AuthoritySetId, DKGReport, MisbehaviourType,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	gossip_messages::MisbehaviourMessage,
	utils::to_slice_33,
	AggregatedMisbehaviourReports, AggregatedPublicKeys, AuthoritySet, BatchId, DKGApi,
	MaxAuthorities, MaxProposalLength, MaxProposalsInBatch, MaxReporters, MaxSignatureLength,
	GENESIS_AUTHORITY_SET_ID,
};

use crate::{
	async_protocols::{
		remote::AsyncProtocolRemote, AsyncProtocolParameters, GenericAsyncHandler, KeygenRound,
	},
	error,
	gossip_engine::GossipEngineIface,
	gossip_messages::{
		misbehaviour_report::{gossip_misbehaviour_report, handle_misbehaviour_report},
		public_key_gossip::handle_public_key_broadcast,
	},
	keygen_manager::{KeygenManager, KeygenState},
	keystore::DKGKeystore,
	metric_inc, metric_set,
	metrics::Metrics,
	utils::{find_authorities_change, SendFuture},
	Client,
};

pub const ENGINE_ID: sp_runtime::ConsensusEngineId = *b"WDKG";

pub const STORAGE_SET_RETRY_NUM: usize = 5;

pub const MAX_SUBMISSION_DELAY: u32 = 3;

pub const MAX_KEYGEN_RETRIES: usize = 5;

/// How many blocks to keep the proposal hash in out local cache.
pub const PROPOSAL_HASH_LIFETIME: u32 = 10;

pub type Shared<T> = Arc<RwLock<T>>;

pub struct WorkerParams<B, BE, C, GE>
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
	pub network: Option<Arc<NetworkService<B, B::Hash>>>,
	pub sync_service: Option<Arc<SyncingService<B>>>,
	pub test_bundle: Option<TestBundle>,
	pub _marker: PhantomData<B>,
}

/// A DKG worker plays the DKG protocol
pub struct DKGWorker<B, BE, C, GE>
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
	/// Cached best authorities
	pub best_authorities: Shared<Vec<(u16, Public)>>,
	/// Cached next best authorities
	pub next_best_authorities: Shared<Vec<(u16, Public)>>,
	/// Latest block header
	pub latest_header: Shared<Option<B::Header>>,
	/// Current validator set
	pub current_validator_set: Shared<AuthoritySet<Public, MaxAuthorities>>,
	/// Queued validator set
	pub queued_validator_set: Shared<AuthoritySet<Public, MaxAuthorities>>,
	/// Tracking for the broadcasted public keys and signatures
	pub aggregated_public_keys: Shared<AggregatedPublicKeysAndSigs>,
	/// Tracking for the misbehaviour reports
	pub aggregated_misbehaviour_reports: Shared<AggregatedMisbehaviourReportStore>,
	/// Concrete type that points to the actual local keystore if it exists
	pub local_keystore: Shared<Option<Arc<LocalKeystore>>>,
	/// Used to keep track of network status
	pub network: Option<Arc<NetworkService<B, B::Hash>>>,
	/// Used to keep track of sync status
	pub sync_service: Option<Arc<SyncingService<B>>>,
	pub test_bundle: Option<TestBundle>,
	pub logger: DebugLogger,
	pub signing_manager: SigningManager<B, BE, C, GE>,
	pub keygen_manager: KeygenManager<B, BE, C, GE>,
	pub(crate) error_handler_channel: ErrorHandlerChannel,
	// keep rustc happy
	_backend: PhantomData<(BE, MaxProposalLength)>,
}

#[derive(Clone)]
pub(crate) struct ErrorHandlerChannel {
	pub tx: tokio::sync::mpsc::UnboundedSender<DKGError>,
	rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<DKGError>>>>,
}

/// Used only for tests
#[derive(Clone)]
pub struct TestBundle {
	pub to_test_client: UnboundedSender<TestClientPayload>,
	pub current_test_id: Arc<RwLock<Option<uuid::Uuid>>>,
}

pub type TestClientPayload = (uuid::Uuid, Result<(), String>, Option<Vec<u8>>);

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
			best_authorities: self.best_authorities.clone(),
			next_best_authorities: self.next_best_authorities.clone(),
			latest_header: self.latest_header.clone(),
			current_validator_set: self.current_validator_set.clone(),
			queued_validator_set: self.queued_validator_set.clone(),
			aggregated_public_keys: self.aggregated_public_keys.clone(),
			aggregated_misbehaviour_reports: self.aggregated_misbehaviour_reports.clone(),
			local_keystore: self.local_keystore.clone(),
			test_bundle: self.test_bundle.clone(),
			network: self.network.clone(),
			sync_service: self.sync_service.clone(),
			logger: self.logger.clone(),
			signing_manager: self.signing_manager.clone(),
			keygen_manager: self.keygen_manager.clone(),
			error_handler_channel: self.error_handler_channel.clone(),
			_backend: PhantomData,
		}
	}
}

pub type AggregatedPublicKeysAndSigs = HashMap<SessionId, AggregatedPublicKeys>;

pub type AggregatedMisbehaviourReportStore = HashMap<
	(MisbehaviourType, SessionId, AuthorityId),
	AggregatedMisbehaviourReports<AuthorityId, MaxSignatureLength, MaxReporters>,
>;

impl<B, BE, C, GE> DKGWorker<B, BE, C, GE>
where
	B: Block + Codec,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	/// Return a new DKG worker instance.
	///
	/// Note that a DKG worker is only fully functional if a corresponding
	/// DKG pallet has been deployed on-chain.
	///
	/// The DKG pallet is needed in order to keep track of the DKG authority set.
	pub fn new(worker_params: WorkerParams<B, BE, C, GE>, logger: DebugLogger) -> Self {
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
			network,
			sync_service,
			test_bundle,
			..
		} = worker_params;

		let clock = Clock { latest_header: latest_header.clone() };
		let signing_manager = SigningManager::<B, BE, C, GE>::new(logger.clone(), clock.clone());
		// 2 tasks max: 1 for current, 1 for queued
		let keygen_manager = KeygenManager::new(logger.clone(), clock);

		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		let error_handler_channel = ErrorHandlerChannel { tx, rx: Arc::new(Mutex::new(Some(rx))) };

		DKGWorker {
			client,
			backend,
			key_store,
			db: db_backend,
			keygen_manager,
			keygen_gossip_engine: Arc::new(keygen_gossip_engine),
			signing_gossip_engine: Arc::new(signing_gossip_engine),
			metrics: Arc::new(metrics),
			best_authorities: Arc::new(RwLock::new(vec![])),
			next_best_authorities: Arc::new(RwLock::new(vec![])),
			current_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			queued_validator_set: Arc::new(RwLock::new(AuthoritySet::empty())),
			latest_header,
			aggregated_public_keys: Arc::new(RwLock::new(HashMap::new())),
			aggregated_misbehaviour_reports: Arc::new(RwLock::new(HashMap::new())),
			local_keystore: Arc::new(RwLock::new(local_keystore)),
			test_bundle,
			error_handler_channel,
			logger,
			network,
			sync_service,
			signing_manager,
			_backend: PhantomData,
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProtoStageType {
	KeygenGenesis,
	KeygenStandard,
	Signing { unsigned_proposal_hash: [u8; 32] },
}

#[derive(Debug, Copy, Clone)]
pub struct AnticipatedKeygenExecutionStatus {
	pub execute: bool,
	pub force_execute: bool,
}

impl<B, BE, C, GE> DKGWorker<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	// NOTE: This must be ran at the start of each epoch since best_authorities may change
	// if "current" is true, this will set the "rounds" field in the dkg worker, otherwise,
	// it well set the "next_rounds" field
	#[allow(clippy::too_many_arguments, clippy::type_complexity)]
	pub(crate) fn generate_async_proto_params(
		&self,
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		stage: ProtoStageType,
		protocol_name: &str,
		associated_block: NumberFor<B>,
		ssid: u8,
	) -> Result<
		AsyncProtocolParameters<
			DKGProtocolEngine<
				B,
				BE,
				C,
				GE,
				MaxProposalLength,
				MaxAuthorities,
				BatchId,
				MaxProposalsInBatch,
				MaxSignatureLength,
			>,
			MaxAuthorities,
		>,
		DKGError,
	> {
		let best_authorities = Arc::new(best_authorities);
		let authority_public_key = Arc::new(authority_public_key);

		let now = self.get_latest_block_number();
		let associated_block_id: u64 = associated_block.saturated_into();
		let status_handle = AsyncProtocolRemote::new(
			now,
			session_id,
			self.logger.clone(),
			associated_block_id,
			ssid,
		);
		// Fetch the active key. This requires rotating the key to have happened with
		// full certainty in order to ensure the right key is being used to make signatures.
		let active_local_key = match stage {
			ProtoStageType::KeygenGenesis => None,
			ProtoStageType::KeygenStandard => None,
			ProtoStageType::Signing { .. } => {
				let (active_local_key, _) = self.fetch_local_keys(session_id);
				active_local_key
			},
		};
		self.logger.debug(format!(
			"Active local key enabled for stage {:?}? {}",
			stage,
			active_local_key.is_some()
		));

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
				is_genesis: stage == ProtoStageType::KeygenGenesis,
				metrics: self.metrics.clone(),
				test_bundle: self.test_bundle.clone(),
				logger: self.logger.clone(),
				_pd: Default::default(),
			}),
			session_id,
			db: self.db.clone(),
			keystore: self.key_store.clone(),
			current_validator_set: self.current_validator_set.clone(),
			best_authorities,
			party_i,
			authority_public_key,
			batch_id_gen: Arc::new(Default::default()),
			handle: status_handle,
			logger: self.logger.clone(),
			local_key: active_local_key,
			associated_block_id,
		};

		match &stage {
			ProtoStageType::Signing { unsigned_proposal_hash } => {
				self.logger.debug(format!("Signing protocol for proposal hash {unsigned_proposal_hash:?} will start later in the signing manager"));
				Ok(params)
			},

			ProtoStageType::KeygenGenesis | ProtoStageType::KeygenStandard => {
				self.logger.debug(format!(
					"Protocol for stage {stage:?} will start later in the keygen manager"
				));
				Ok(params)
			},
		}
	}

	/// Returns the gossip engine based on the protocol_name
	fn get_gossip_engine_from_protocol_name(&self, protocol_name: &str) -> Arc<GE> {
		match protocol_name {
			crate::DKG_KEYGEN_PROTOCOL_NAME => self.keygen_gossip_engine.clone(),
			crate::DKG_SIGNING_PROTOCOL_NAME => self.signing_gossip_engine.clone(),
			_ => panic!("Protocol name not found!"),
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) async fn initialize_keygen_protocol(
		&self,
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		associated_block: NumberFor<B>,
		threshold: u16,
		stage: ProtoStageType,
		keygen_protocol_hash: [u8; 32],
	) -> Option<(AsyncProtocolRemote<NumberFor<B>>, Pin<Box<dyn SendFuture<'static, ()>>>)> {
		// There is only ever 1 signing set, implicitly, for keygen (i.e., the list of best
		// authorities)
		const KEYGEN_SSID: u8 = 0;
		match self.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			stage,
			crate::DKG_KEYGEN_PROTOCOL_NAME,
			associated_block,
			KEYGEN_SSID,
		) {
			Ok(async_proto_params) => {
				let err_handler_tx = self.error_handler_channel.tx.clone();

				let remote = async_proto_params.handle.clone();
				let keygen_manager = self.keygen_manager.clone();
				let status = match stage {
					ProtoStageType::KeygenGenesis => KeygenRound::Genesis,
					ProtoStageType::KeygenStandard => KeygenRound::Next,
					ProtoStageType::Signing { .. } => {
						unreachable!("Should not happen here")
					},
				};

				match GenericAsyncHandler::setup_keygen(
					async_proto_params,
					threshold,
					status,
					keygen_protocol_hash,
				) {
					Ok(meta_handler) => {
						let logger = self.logger.clone();
						let signing_manager = self.signing_manager.clone();
						signing_manager.keygen_lock();
						let task = async move {
							match meta_handler.await {
								Ok(_) => {
									keygen_manager.set_state(KeygenState::KeygenCompleted {
										session_completed: session_id,
									});
									let _ = keygen_manager
										.finished_count
										.fetch_add(1, Ordering::SeqCst);
									signing_manager.keygen_unlock();
									logger.info(
										"The keygen meta handler has executed successfully"
											.to_string(),
									);

									Ok(())
								},

								Err(err) => {
									logger
										.error(format!("Error executing meta handler {:?}", &err));
									keygen_manager.set_state(KeygenState::Failed { session_id });
									signing_manager.keygen_unlock();
									let _ = err_handler_tx.send(err.clone());
									Err(err)
								},
							}
						};

						self.logger.debug(format!("Created Keygen Protocol task for session {session_id} with status {status:?}"));
						return Some((remote, Box::pin(task)))
					},

					Err(err) => {
						self.logger.error(format!("Error starting meta handler {:?}", &err));
						self.handle_dkg_error(err).await;
					},
				}
			},

			Err(err) => {
				self.handle_dkg_error(err).await;
			},
		}

		None
	}

	/// Fetch the stored local keys if they exist.
	fn fetch_local_keys(
		&self,
		current_session_id: SessionId,
	) -> (Option<LocalKey<Secp256k1>>, Option<LocalKey<Secp256k1>>) {
		let next_session_id = current_session_id + 1;
		let active_local_key = self.db.get_local_key(current_session_id).ok().flatten();
		let next_local_key = self.db.get_local_key(next_session_id).ok().flatten();
		(active_local_key, next_local_key)
	}

	/// Get the party index of our worker
	///
	/// Returns `None` if we are not in the best authority set
	pub async fn get_party_index(&self, header: &B::Header) -> Option<u16> {
		let public = self.get_authority_public_key();
		let best_authorities = self.get_best_authorities(header).await;
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
	pub async fn get_next_party_index(&self, header: &B::Header) -> Option<u16> {
		let public = self.get_authority_public_key();
		let next_best_authorities = self.get_next_best_authorities(header).await;
		for elt in next_best_authorities {
			if elt.1 == public {
				return Some(elt.0)
			}
		}

		None
	}

	/// Get the signature threshold at a specific block
	pub async fn get_signature_threshold(&self, header: &B::Header) -> u16 {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().signature_threshold(at).unwrap_or_default()
		})
		.await
	}

	/// Get the next signature threshold at a specific block
	pub async fn get_next_signature_threshold(&self, header: &B::Header) -> u16 {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().next_signature_threshold(at).unwrap_or_default()
		})
		.await
	}

	/// Get the active DKG public key
	pub async fn get_dkg_pub_key(&self, header: &B::Header) -> (AuthoritySetId, Vec<u8>) {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().dkg_pub_key(at).unwrap_or_default()
		})
		.await
	}

	pub async fn dkg_pub_key_is_unset(&self, header: &B::Header) -> bool {
		self.get_dkg_pub_key(header).await.1.is_empty()
	}

	/// Get the next DKG public key
	pub async fn get_next_dkg_pub_key(
		&self,
		header: &B::Header,
	) -> Option<(AuthoritySetId, Vec<u8>)> {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().next_dkg_pub_key(at).unwrap_or_default()
		})
		.await
	}

	/// Get the jailed keygen authorities
	#[allow(dead_code)]
	pub async fn get_keygen_jailed(
		&self,
		header: &B::Header,
		set: &[AuthorityId],
	) -> Vec<AuthorityId> {
		let at = header.hash();
		let set = set.to_vec();
		self.exec_client_function(move |client| {
			client.runtime_api().get_keygen_jailed(at, set).unwrap_or_default()
		})
		.await
	}

	/// Get the best authorities for keygen
	pub async fn get_best_authorities(&self, header: &B::Header) -> Vec<(u16, AuthorityId)> {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().get_best_authorities(at).unwrap_or_default()
		})
		.await
	}

	/// Get the next best authorities for keygen
	pub async fn get_next_best_authorities(&self, header: &B::Header) -> Vec<(u16, AuthorityId)> {
		let at = header.hash();
		self.exec_client_function(move |client| {
			client.runtime_api().get_next_best_authorities(at).unwrap_or_default()
		})
		.await
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
	pub async fn validator_set(
		&self,
		header: &B::Header,
	) -> Option<(AuthoritySet<Public, MaxAuthorities>, AuthoritySet<Public, MaxAuthorities>)> {
		Self::validator_set_inner(&self.logger, header, &self.client).await
	}

	async fn validator_set_inner(
		logger: &DebugLogger,
		header: &B::Header,
		client: &Arc<C>,
	) -> Option<(AuthoritySet<Public, MaxAuthorities>, AuthoritySet<Public, MaxAuthorities>)> {
		let new = if let Some((new, queued)) = find_authorities_change::<B>(header) {
			Some((new, queued))
		} else {
			let at = header.hash();
			let current_authority_set = exec_client_function(client, move |client| {
				client.runtime_api().authority_set(at).ok()
			})
			.await;

			let queued_authority_set = exec_client_function(client, move |client| {
				client.runtime_api().queued_authority_set(at).ok()
			})
			.await;

			match (current_authority_set, queued_authority_set) {
				(Some(current), Some(queued)) => Some((current, queued)),
				_ => None,
			}
		};

		logger.trace(format!("🕸️  active validator set: {new:?}"));

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
		mut active: AuthoritySet<Public, MaxAuthorities>,
	) -> Result<(), error::Error> {
		let active: BTreeSet<Public> = active.authorities.drain(..).collect();

		let store: BTreeSet<Public> = self.key_store.public_keys()?.drain(..).collect();

		let missing: Vec<_> = store.difference(&active).cloned().collect();

		if !missing.is_empty() {
			self.logger.debug(format!(
				"🕸️  for block {block:?}, public key missing in validator set is: {missing:?}"
			));
		}

		Ok(())
	}

	// *** Block notifications ***
	async fn process_block_notification(&self, header: &B::Header) {
		if let Some(latest_header) = self.latest_header.read().clone() {
			if latest_header.number() >= header.number() {
				// We've already seen this block, ignore it.
				self.logger.debug(
					format!("🕸️  Latest header {} is greater than or equal to current header {}, returning...",
					latest_header.number(),
					header.number()
					)
				);
				return
			}
		}
		self.logger
			.debug(format!("🕸️  Processing block notification for block {}", header.number()));
		metric_set!(self, dkg_latest_block_height, header.number());
		*self.latest_header.write() = Some(header.clone());
		self.logger.debug(format!("🕸️  Latest header is now: {:?}", header.number()));

		// if we are still syncing, return immediately

		if let Some(sync_service) = &self.sync_service {
			if sync_service.is_major_syncing() {
				self.logger.debug("🕸️  Chain not fully synced, skipping block processing!");
				return
			}
		}

		// Attempt to enact new DKG authorities if sessions have changed
		// The Steps for enacting new DKG authorities are:
		// 1. Check if the DKG Public Key are not yet set on chain (or not yet generated)
		// 2. if yes, we start enacting authorities on genesis flow.
		// 3. if no, we start enacting authorities on queued flow and submit any unsigned proposals.
		if self.dkg_pub_key_is_unset(header).await {
			self.logger
				.debug("🕸️  Maybe enacting genesis authorities since dkg pub key is empty");
			self.maybe_enact_genesis_authorities(header).await;
			self.keygen_manager.on_block_finalized(header, self).await;
		} else {
			// maybe update the internal state of the worker
			self.maybe_update_worker_state(header).await;
			self.keygen_manager.on_block_finalized(header, self).await;
			if let Err(e) = self.signing_manager.on_block_finalized(header, self).await {
				self.logger
					.error(format!("🕸️  Error running signing_manager.on_block_finalized: {e:?}"));
			}
		}
	}

	async fn maybe_enact_genesis_authorities(&self, header: &B::Header) {
		// Get the active and queued validators to check for updates
		if let Some((active, _queued)) = self.validator_set(header).await {
			// If we are in the genesis state, we need to enact the genesis authorities
			if active.id == GENESIS_AUTHORITY_SET_ID {
				self.logger.debug(format!("🕸️  GENESIS SESSION ID {:?}", active.id));
				metric_set!(self, dkg_validator_set_id, active.id);
				// verify the new validator set
				let _ = self.verify_validator_set(header.number(), active.clone());
				// Setting new validator set id as current
				*self.current_validator_set.write() = active.clone();
				*self.best_authorities.write() = self.get_best_authorities(header).await;
				*self.next_best_authorities.write() = self.get_next_best_authorities(header).await;
			} else {
				self.logger.debug(format!("🕸️  NOT IN GENESIS SESSION ID {:?}", active.id));
			}
		} else {
			self.logger.debug("🕸️  No active validators");
		}
	}

	async fn maybe_update_worker_state(&self, header: &B::Header) {
		if let Some((active, queued)) = self.validator_set(header).await {
			self.logger.debug(format!("🕸️  ACTIVE SESSION ID {:?}", active.id));
			metric_set!(self, dkg_validator_set_id, active.id);
			// verify the new validator set
			let _ = self.verify_validator_set(header.number(), active.clone());
			// Check if the on chain authority_set_id is the same as the queued_authority_set_id.
			let (set_id, _) = self.get_dkg_pub_key(header).await;
			let queued_authority_set_id = self.queued_validator_set.read().id;
			self.logger.debug(format!("🕸️  CURRENT SET ID: {set_id:?}"));
			self.logger
				.debug(format!("🕸️  QUEUED AUTHORITY SET ID: {queued_authority_set_id:?}"));
			if set_id != queued_authority_set_id {
				self.logger.debug(format!("🕸️  Queued authority set id {queued_authority_set_id} is not the same as the on chain authority set id {set_id}, will not rotate the local sessions."));
				return
			}
			// Update the validator sets
			*self.current_validator_set.write() = active;
			*self.queued_validator_set.write() = queued;
			// We also rotate the best authority caches
			*self.best_authorities.write() = self.next_best_authorities.read().clone();
			*self.next_best_authorities.write() = self.get_next_best_authorities(header).await;
			// Reset per session metrics
			if let Some(metrics) = self.metrics.as_ref() {
				metrics.reset_session_metrics();
			}
		} else {
			self.logger.info(
				"🕸️  No update to local session found, not rotating local sessions".to_string(),
			);
		}
	}

	async fn handle_finality_notification(&self, notification: FinalityNotification<B>) {
		self.logger.trace(format!("🕸️  Finality notification: {notification:?}"));
		// Handle finality notifications
		self.process_block_notification(&notification.header).await;
	}

	#[cfg_attr(
		feature = "debug-tracing",
		dkg_logging::instrument(target = "dkg", skip_all, ret, err, fields(signed_dkg_message))
	)]
	async fn verify_signature_against_authorities(
		&self,
		signed_dkg_msg: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError> {
		Self::verify_signature_against_authorities_inner(
			&self.logger,
			signed_dkg_msg,
			&self.latest_header,
			&self.client,
		)
		.await
	}

	pub async fn verify_signature_against_authorities_inner(
		logger: &DebugLogger,
		signed_dkg_msg: SignedDKGMessage<Public>,
		latest_header: &Arc<RwLock<Option<B::Header>>>,
		client: &Arc<C>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let dkg_msg = signed_dkg_msg.msg;
		let encoded = dkg_msg.encode();
		let signature = signed_dkg_msg.signature.ok_or(DKGError::GenericError {
			reason: "Signature not found in signed_dkg_msg".into(),
		})?;
		// Get authority accounts
		let mut authorities: Option<(Vec<AuthorityId>, Vec<AuthorityId>)> = None;
		let latest_header = { latest_header.read().clone() };

		if let Some(header) = latest_header {
			authorities = Self::validator_set_inner(logger, &header, client)
				.await
				.map(|a| (a.0.authorities.into(), a.1.authorities.into()));
		}

		if authorities.is_none() {
			return Err(DKGError::GenericError { reason: "No authorities".into() })
		}

		let check_signers = |xs: &[AuthorityId]| {
			return dkg_runtime_primitives::utils::verify_signer_from_set_ecdsa(
				xs.iter()
					.map(|x| {
						let slice_33 =
							to_slice_33(&x.encode()).expect("AuthorityId encoding failed!");
						ecdsa::Public::from_raw(slice_33)
					})
					.collect(),
				&encoded,
				&signature,
			)
			.1
		};

		if check_signers(&authorities.clone().expect("Checked for empty authorities above").0) ||
			check_signers(&authorities.expect("Checked for empty authorities above").1)
		{
			Ok(dkg_msg)
		} else {
			Err(DKGError::GenericError {
				reason: "Message signature is not from a registered authority or next authority"
					.into(),
			})
		}
	}

	#[cfg_attr(
		feature = "debug-tracing",
		dkg_logging::instrument(target = "dkg", skip_all, fields(dkg_error))
	)]
	pub async fn handle_dkg_error(&self, dkg_error: DKGError) {
		self.logger.error(format!("Received error: {dkg_error:?}"));
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

		self.logger
			.error(format!("Bad Actors : {bad_actors:?}, Session Id : {session_id:?}"));

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
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender, session_id })
						.await,
				DKGError::KeygenTimeout { .. } =>
					self.handle_dkg_report(DKGReport::KeygenMisbehaviour { offender, session_id })
						.await,
				DKGError::SignMisbehaviour { bad_actors: _, .. } =>
					self.handle_dkg_report(DKGReport::SignMisbehaviour { offender, session_id })
						.await,
				_ => (),
			}
		}
	}

	/// Route messages internally where they need to be routed
	#[cfg_attr(
		feature = "debug-tracing",
		dkg_logging::instrument(target = "dkg", skip_all, ret, err, fields(dkg_msg))
	)]
	async fn process_incoming_dkg_message(
		&self,
		dkg_msg: SignedDKGMessage<Public>,
	) -> Result<(), DKGError> {
		metric_inc!(self, dkg_inbound_messages);
		self.logger
			.info(format!("Processing incoming DKG message: {:?}", dkg_msg.msg.session_id,));

		match &dkg_msg.msg.payload {
			NetworkMsgPayload::Keygen(_) => {
				self.keygen_manager.deliver_message(dkg_msg);
				Ok(())
			},
			NetworkMsgPayload::Offline(..) | NetworkMsgPayload::Vote(..) => {
				self.signing_manager.deliver_message(dkg_msg);
				Ok(())
			},
			NetworkMsgPayload::PublicKeyBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg).await {
					Ok(dkg_msg) => {
						match handle_public_key_broadcast(self, dkg_msg).await {
							Ok(()) => (),
							Err(err) => self
								.logger
								.error(format!("🕸️  Error while handling DKG message {err:?}")),
						};
					},

					Err(err) => self.logger.error(format!(
						"Error while verifying signature against authorities: {err:?}"
					)),
				}
				Ok(())
			},
			NetworkMsgPayload::MisbehaviourBroadcast(_) => {
				match self.verify_signature_against_authorities(dkg_msg).await {
					Ok(dkg_msg) => {
						match handle_misbehaviour_report(self, dkg_msg).await {
							Ok(()) => (),
							Err(err) => self.logger.error(format!(
								"🕸️  Error while handling misbehaviour message {err:?}"
							)),
						};
					},

					Err(err) => self.logger.error(format!(
						"Error while verifying signature against authorities: {err:?}"
					)),
				}

				Ok(())
			},
		}
	}

	async fn handle_dkg_report(&self, dkg_report: DKGReport) {
		let (offender, session_id, misbehaviour_type) = match dkg_report {
			// Keygen misbehaviour possibly leads to keygen failure. This should be slashed
			// more severely than sign misbehaviour events.
			DKGReport::KeygenMisbehaviour { offender, session_id } => {
				self.logger.info(format!(
					"🕸️  DKG Keygen misbehaviour @ Session ({session_id}) by {offender}"
				));
				(offender, session_id, MisbehaviourType::Keygen)
			},
			DKGReport::SignMisbehaviour { offender, session_id } => {
				self.logger.info(format!(
					"🕸️  DKG Signing misbehaviour @ Session ({session_id}) by {offender}"
				));
				(offender, session_id, MisbehaviourType::Sign)
			},
		};

		let misbehaviour_msg =
			MisbehaviourMessage { misbehaviour_type, session_id, offender, signature: vec![] };
		let gossip = gossip_misbehaviour_report(self, misbehaviour_msg).await;
		if gossip.is_err() {
			self.logger.info("🕸️  DKG gossip_misbehaviour_report failed!");
		}
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

		let signer = maybe_signer.ok_or(DKGError::GenericError {
			reason: "verify_signer_from_set_ecdsa could not determin signer!".to_string(),
		})?;

		Ok(Public::from(signer))
	}

	fn get_jailed_signers_inner(
		&self,
		best_authorities: &[Public],
	) -> Result<Vec<Public>, DKGError> {
		let now = self.latest_header.read().clone().ok_or_else(|| DKGError::CriticalError {
			reason: "latest header does not exist!".to_string(),
		})?;
		let at = now.hash();
		Ok(self
			.client
			.runtime_api()
			.get_signing_jailed(at, best_authorities.to_vec())
			.unwrap_or_default())
	}
	pub(crate) fn get_unjailed_signers(
		&self,
		best_authorities: &[Public],
	) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner(best_authorities)?;
		Ok(best_authorities
			.iter()
			.enumerate()
			.filter(|(_, key)| !jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}

	/// Get the jailed signers
	pub(crate) fn get_jailed_signers(
		&self,
		best_authorities: &[Public],
	) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner(best_authorities)?;
		Ok(best_authorities
			.iter()
			.enumerate()
			.filter(|(_, key)| jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}

	pub async fn should_execute_new_keygen(
		&self,
		header: &B::Header,
	) -> AnticipatedKeygenExecutionStatus {
		// query runtime api to check if we should execute new keygen.
		let at = header.hash();
		let (execute, force_execute) = self
			.exec_client_function(move |client| {
				client.runtime_api().should_execute_new_keygen(at).unwrap_or_default()
			})
			.await;

		AnticipatedKeygenExecutionStatus { execute, force_execute }
	}

	/// Wraps the call in a SpawnBlocking task
	pub async fn exec_client_function<F, T>(&self, function: F) -> T
	where
		for<'a> F: FnOnce(&'a C) -> T,
		T: Send + 'static,
		F: Send + 'static,
	{
		let client = &self.client;
		exec_client_function(client, function).await
	}

	/// Wait for initial finalized block
	async fn initialization(&mut self) {
		let mut stream = self.client.finality_notification_stream();
		while let Some(notif) = stream.next().await {
			if let Some((active, queued)) = self.validator_set(&notif.header).await {
				// Cache the authority sets and best authorities
				*self.best_authorities.write() = self.get_best_authorities(&notif.header).await;
				*self.current_validator_set.write() = active;
				*self.queued_validator_set.write() = queued;
				// Route this to the finality notification handler
				self.handle_finality_notification(notif.clone()).await;
				self.logger.debug("Initialization complete");
				// End the initialization stream
				return
			}
		}
	}

	// *** Main run loop ***
	pub async fn run(mut self) {
		crate::deadlock_detection::deadlock_detect();
		self.initialization().await;

		self.logger.debug("Starting DKG Iteration loop");
		// We run all these tasks in parallel and wait for any of them to complete.
		// If any of them completes, we stop all the other tasks since this means a fatal error has
		// occurred and we need to shut down.
		let (first, n, ..) = futures::future::select_all(vec![
			self.spawn_finality_notification_task(),
			self.spawn_keygen_messages_stream_task(),
			self.spawn_signing_messages_stream_task(),
			self.spawn_error_handling_task(),
		])
		.await;
		self.logger
			.error(format!("DKG Worker finished; the reason that task({n}) ended with: {first:?}"));
	}

	fn spawn_finality_notification_task(&self) -> tokio::task::JoinHandle<()> {
		let mut stream = self.client.finality_notification_stream();
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(notification) = stream.next().await {
				dkg_logging::debug!("Going to handle Finality notification");
				self_.handle_finality_notification(notification).await;
			}

			self_.logger.error("Finality notification stream ended");
		})
	}

	fn spawn_keygen_messages_stream_task(&self) -> tokio::task::JoinHandle<()> {
		let keygen_gossip_engine = self.keygen_gossip_engine.clone();
		let mut keygen_stream =
			keygen_gossip_engine.get_stream().expect("keygen gossip stream already taken");
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(msg) = keygen_stream.recv().await {
				let msg_hash = crate::debug_logger::raw_message_to_hash(msg.msg.payload.payload());
				self_.logger.debug(format!(
					"Going to handle keygen message for session {} | hash: {msg_hash}",
					msg.msg.session_id
				));
				self_.logger.checkpoint_message_raw(msg.msg.payload.payload(), "CP1-keygen");
				match self_.process_incoming_dkg_message(msg).await {
					Ok(_) => {},
					Err(e) => {
						self_.logger.error(format!("Error processing keygen message: {e:?}"));
					},
				}
			}
		})
	}

	fn spawn_signing_messages_stream_task(&self) -> tokio::task::JoinHandle<()> {
		let signing_gossip_engine = self.signing_gossip_engine.clone();
		let mut signing_stream =
			signing_gossip_engine.get_stream().expect("signing gossip stream already taken");
		let self_ = self.clone();
		tokio::spawn(async move {
			while let Some(msg) = signing_stream.recv().await {
				self_.logger.debug(format!(
					"Going to handle signing message for session {}",
					msg.msg.session_id
				));
				self_.logger.checkpoint_message_raw(msg.msg.payload.payload(), "CP1-signing");
				match self_.process_incoming_dkg_message(msg).await {
					Ok(_) => {},
					Err(e) => {
						self_.logger.error(format!("Error processing signing message: {e:?}"));
					},
				}
			}
		})
	}

	fn spawn_error_handling_task(&self) -> tokio::task::JoinHandle<()> {
		let self_ = self.clone();
		let mut error_handler_rx = self
			.error_handler_channel
			.rx
			.lock()
			.take()
			.expect("Error handler tx already taken");
		let logger = self.logger.clone();
		tokio::spawn(async move {
			while let Some(error) = error_handler_rx.recv().await {
				logger.debug("Going to handle Error");
				self_.handle_dkg_error(error).await;
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
			.authority_id(
				&self.get_keystore().public_keys().expect("Could not find authority public key"),
			)
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
	MaxProposalLength: Get<u32>,
	MaxAuthorities: Get<u32>,
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
pub trait HasLatestHeader<B: Block>: Send + Sync + 'static {
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
	BE: Backend<B> + 'static,
	GE: GossipEngineIface,
	C: Client<B, BE> + 'static,
	MaxProposalLength: Get<u32>,
	MaxAuthorities: Get<u32>,
{
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}

	#[doc = " Gets latest block number from latest block header"]
	fn get_latest_block_number(&self) -> NumberFor<B> {
		if let Some(latest_header) = self.get_latest_header().read().clone() {
			*latest_header.number()
		} else {
			NumberFor::<B>::from(0u32)
		}
	}
}

pub struct Clock<B: Block> {
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
}

impl<B: Block> Clone for Clock<B> {
	fn clone(&self) -> Self {
		Self { latest_header: self.latest_header.clone() }
	}
}

impl<B: Block> HasLatestHeader<B> for Clock<B> {
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}
}

/// Wraps the call in a SpawnBlocking task
async fn exec_client_function<B, C, BE, F, T>(client: &Arc<C>, function: F) -> T
where
	for<'a> F: FnOnce(&'a C) -> T,
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE> + 'static,
	T: Send + 'static,
	F: Send + 'static,
{
	let client = client.clone();
	tokio::task::spawn_blocking(move || function(&client))
		.await
		.expect("Failed to spawn blocking task")
}
