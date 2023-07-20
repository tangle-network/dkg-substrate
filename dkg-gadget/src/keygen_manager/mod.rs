#![allow(clippy::needless_return)]

use crate::{
	async_protocols::{remote::AsyncProtocolRemote, KeygenPartyId, KeygenRound},
	dkg_modules::KeygenProtocolSetupParameters,
	gossip_engine::GossipEngineIface,
	signing_manager::work_manager::{JobMetadata, PollMethod, WorkManager},
	utils::SendFuture,
	worker::{
		AnticipatedKeygenExecutionStatus, DKGWorker, HasLatestHeader, KeystoreExt, ProtoStageType,
	},
	Client,
};
use atomic::Atomic;
use dkg_logging::debug_logger::DebugLogger;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	keccak_256, DKGApi, MaxAuthorities, MaxProposalLength, SessionId, GENESIS_AUTHORITY_SET_ID,
};
use sc_client_api::Backend;
use sp_arithmetic::traits::SaturatedConversion;
use sp_runtime::traits::{Block, Header, NumberFor};
use std::{
	marker::PhantomData,
	pin::Pin,
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	},
};

/// The KeygenManager is an abstraction that manages the lifecycle for executing and maintaining
/// keygen protocols. Code for this use to previously live in the DKGWorker, but has now been moved
/// here for readability and maintainability.
pub struct KeygenManager<B: Block, BE, C, GE> {
	// governs the workload for each node
	work_manager: WorkManager<B>,
	active_keygen_retry_id: Arc<AtomicUsize>,
	keygen_state: Arc<Atomic<KeygenState>>,
	latest_executed_session_id: Arc<Atomic<Option<SessionId>>>,
	pub finished_count: Arc<AtomicUsize>,
	_pd: PhantomData<(B, BE, C, GE)>,
}

impl<B: Block, BE, C, GE> Clone for KeygenManager<B, BE, C, GE> {
	fn clone(&self) -> Self {
		Self {
			work_manager: self.work_manager.clone(),
			_pd: self._pd,
			active_keygen_retry_id: self.active_keygen_retry_id.clone(),
			keygen_state: self.keygen_state.clone(),
			latest_executed_session_id: self.latest_executed_session_id.clone(),
			finished_count: self.finished_count.clone(),
		}
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// State of the KeygenManager
pub enum KeygenState {
	Uninitialized,
	RunningKeygen,
	RunningGenesisKeygen,
	// session_completed denotes the session that executed the keygen, NOT
	// the generated DKG public key for the next session
	KeygenCompleted { session_completed: u64 },
	Failed { session_id: u64 },
}

/// only 1 task at a time may run for keygen
const MAX_RUNNING_TASKS: usize = 1;
/// There should never be any job enqueueing for keygen
const MAX_ENQUEUED_TASKS: usize = 0;

impl<B, BE, C, GE> KeygenManager<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	pub fn new(logger: DebugLogger, clock: impl HasLatestHeader<B>) -> Self {
		Self {
			work_manager: WorkManager::<B>::new(
				logger,
				clock,
				MAX_RUNNING_TASKS,
				MAX_ENQUEUED_TASKS,
				PollMethod::Manual,
			),
			active_keygen_retry_id: Arc::new(AtomicUsize::new(0)),
			keygen_state: Arc::new(Atomic::new(KeygenState::Uninitialized)),
			latest_executed_session_id: Arc::new(Atomic::new(None)),
			finished_count: Arc::new(AtomicUsize::new(0)),
			_pd: Default::default(),
		}
	}

	pub fn deliver_message(&self, message: SignedDKGMessage<Public>) {
		let message_task_hash =
			*message.msg.payload.keygen_protocol_hash().expect("Bad message type");
		self.work_manager.deliver_message(message, message_task_hash)
	}

	pub fn session_id_of_active_keygen(&self, now: NumberFor<B>) -> Option<JobMetadata> {
		self.work_manager.get_active_sessions_metadata(now).pop()
	}

	pub fn get_latest_executed_session_id(&self) -> Option<SessionId> {
		self.latest_executed_session_id.load(Ordering::SeqCst)
	}

	fn state(&self) -> KeygenState {
		self.keygen_state.load(Ordering::SeqCst)
	}

	pub fn set_state(&self, state: KeygenState) {
		self.keygen_state.store(state, Ordering::SeqCst);
	}

	/// GENERAL WORKFLOW for Keygen
	///
	/// Session 0 (beginning): immediately run genesis
	/// Session 0 (ending): run keygen for session 1
	/// Session 1 (ending): run keygen for session 2
	/// Session 2 (ending): run keygen for session 3
	/// Session 3 (ending): run keygen for session 4
	#[allow(clippy::needless_return)]
	pub async fn on_block_finalized(
		&self,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) {
		if let Some((active, _queued)) = dkg_worker.validator_set(header).await {
			// Poll to clear any tasks that have finished and to make room for a new potential
			// keygen
			self.work_manager.poll();
			let now_n = *header.number();
			let block_id: u64 = now_n.saturated_into();
			let session_id = active.id;
			let current_protocol = self.session_id_of_active_keygen(now_n);
			let state = self.state();
			let anticipated_execution_status = dkg_worker.should_execute_new_keygen(header).await;
			let executed_count = self.finished_count.load(Ordering::SeqCst);

			dkg_worker.logger.debug(format!(
				"*** KeygenManager on_block_finalized: session={session_id},block={block_id}, state={state:?}, current_protocol={current_protocol:?} | total executed: {executed_count}",
			));
			dkg_worker
				.logger
				.debug(format!("*** Should execute new keygen? {anticipated_execution_status:?}"));

			// Always perform pre-checks
			if self
				.pre_checks(
					session_id,
					state,
					header,
					dkg_worker,
					&current_protocol,
					&anticipated_execution_status,
				)
				.await
			{
				return
			}

			if session_id == GENESIS_AUTHORITY_SET_ID {
				self.genesis_checks(state, header, dkg_worker, &anticipated_execution_status)
					.await;
			} else {
				self.next_checks(
					session_id,
					state,
					header,
					dkg_worker,
					&anticipated_execution_status,
				)
				.await;
			}
		}
	}

	/// Check to see if a genesis keygen failed, or, if we are already running a non-stalled
	/// protocol
	async fn pre_checks(
		&self,
		session_id: u64,
		state: KeygenState,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		current_protocol: &Option<JobMetadata>,
		anticipated_execution: &AnticipatedKeygenExecutionStatus,
	) -> bool {
		if anticipated_execution.force_execute {
			// Unconditionally execute another keygen, overwriting the previous one if necessary
			let stage = if session_id == GENESIS_AUTHORITY_SET_ID &&
				self.finished_count.load(Ordering::SeqCst) == 0
			{
				KeygenRound::Genesis
			} else {
				KeygenRound::Next
			};

			self.maybe_start_keygen_for_stage(stage, header, dkg_worker, anticipated_execution)
				.await;

			return true
		}

		// It's possible genesis failed and we need to retry
		if session_id == GENESIS_AUTHORITY_SET_ID &&
			matches!(state, KeygenState::Failed { session_id: 0 }) &&
			dkg_worker.dkg_pub_key_is_unset(header).await
		{
			dkg_worker
				.logger
				.warn("We will trigger another genesis keygen because the previous one failed");
			self.maybe_start_keygen_for_stage(
				KeygenRound::Genesis,
				header,
				dkg_worker,
				anticipated_execution,
			)
			.await;
			return true
		}

		// If a keygen is already running (and isn't stalled), don't start another one
		if let Some(current_protocol) = current_protocol.as_ref() {
			if current_protocol.is_active && !current_protocol.is_stalled {
				dkg_worker.logger.info("Will not trigger a keygen since one is already running");
				return true
			}
		}

		false
	}

	/// Check to see if we need to run a genesis keygen (session = 0), or, a keygen for session 1
	async fn genesis_checks(
		&self,
		state: KeygenState,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		anticipated_execution: &AnticipatedKeygenExecutionStatus,
	) {
		if state == KeygenState::Uninitialized {
			// If we are at genesis, and there is no active keygen, create and immediately
			// start() one
			return self
				.maybe_start_keygen_for_stage(
					KeygenRound::Genesis,
					header,
					dkg_worker,
					anticipated_execution,
				)
				.await
		}

		if state == KeygenState::RunningGenesisKeygen {
			// If we are at genesis, and a genesis keygen is running, do nothing
			return
		}

		if state == KeygenState::RunningKeygen {
			// If we are at genesis, and there is a next keygen running, do nothing
			return
		}

		if matches!(state, KeygenState::KeygenCompleted { session_completed: 0 }) {
			// If we are at genesis, and we have completed keygen, we may need to begin a keygen
			// for session 1
			return self
				.maybe_start_keygen_for_stage(
					KeygenRound::Next,
					header,
					dkg_worker,
					anticipated_execution,
				)
				.await
		}
	}

	/// Check to see if we need to run a keygen for session 2, 3, 4, .., etc.
	async fn next_checks(
		&self,
		session_id: u64,
		state: KeygenState,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		anticipated_execution: &AnticipatedKeygenExecutionStatus,
	) {
		// Check bad states. These should never happen in a well-behaved program
		if state == KeygenState::RunningGenesisKeygen {
			dkg_worker.logger.error(format!("Invalid keygen manager state: {session_id} > GENESIS_AUTHORITY_SET_ID && {state:?} == KeygenState::GenesisKeygenCompleted || {state:?} == KeygenState::RunningGenesisKeygen"));
			return
		}

		if state == KeygenState::Uninitialized {
			// We joined the network after genesis. We need to start a keygen for session `now`,
			// so long as the next pub key isn't already on chain
			if dkg_worker.get_next_dkg_pub_key(header).await.is_none() {
				self.maybe_start_keygen_for_stage(
					KeygenRound::Next,
					header,
					dkg_worker,
					anticipated_execution,
				)
				.await;
				return
			}
		}

		if state == KeygenState::RunningKeygen {
			// We are in the middle of a keygen. Do nothing
			return
		}

		if matches!(state, KeygenState::KeygenCompleted { .. }) {
			// We maybe need to start a keygen for session `session_id`:
			return self
				.maybe_start_keygen_for_stage(
					KeygenRound::Next,
					header,
					dkg_worker,
					anticipated_execution,
				)
				.await
		}
	}

	async fn maybe_start_keygen_for_stage(
		&self,
		stage: KeygenRound,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		anticipated_execution_status: &AnticipatedKeygenExecutionStatus,
	) {
		let authority_set = if let Some((active, queued)) = dkg_worker.validator_set(header).await {
			match stage {
				KeygenRound::Genesis => active,
				KeygenRound::Next => queued,
			}
		} else {
			return
		};

		let session_id = authority_set.id;
		dkg_worker
			.logger
			.debug(format!("Will attempt to start keygen for session {session_id}"));

		if stage != KeygenRound::Genesis {
			// We need to ensure session progress is close enough to the end to begin execution
			if !anticipated_execution_status.execute && !anticipated_execution_status.force_execute
			{
				dkg_worker.logger.debug("🕸️  Not executing new keygen protocol");
				return
			}

			if dkg_worker.get_next_dkg_pub_key(header).await.is_some() &&
				!anticipated_execution_status.force_execute
			{
				dkg_worker.logger.debug("🕸Not executing new keygen protocol because we already have a next DKG public key");
				return
			}

			if self.finished_count.load(Ordering::SeqCst) != session_id as usize {
				dkg_worker.logger.warn("We have already run this protocol, is this a re-try?");
			}
		} else {
			// if we are in genesis, make sure that the current public key isn't already on-chain
			if !dkg_worker.dkg_pub_key_is_unset(header).await {
				dkg_worker.logger.debug(
					"🕸️  Not executing new keygen protocol because we already have a DKG public key",
				);
				return
			}
		}

		let party_idx = match stage {
			KeygenRound::Genesis => dkg_worker.get_party_index(header).await,
			KeygenRound::Next => dkg_worker.get_next_party_index(header).await,
		};

		let threshold = match stage {
			KeygenRound::Genesis => dkg_worker.get_signature_threshold(header).await,
			KeygenRound::Next => dkg_worker.get_next_signature_threshold(header).await,
		};

		// Check whether the worker is in the best set or return
		let party_i = match party_idx {
			Some(party_index) => {
				dkg_worker.logger.info(format!("🕸️  PARTY {party_index} | SESSION {session_id} | IN THE SET OF BEST AUTHORITIES: session: {session_id} | threshold: {threshold}"));
				if let Ok(res) = KeygenPartyId::try_from(party_index) {
					res
				} else {
					return
				}
			},
			None => {
				dkg_worker
					.logger
					.info(format!("🕸️  NOT IN THE SET OF BEST AUTHORITIES: session: {session_id}"));
				return
			},
		};

		let best_authorities = match stage {
			KeygenRound::Genesis => dkg_worker.get_best_authorities(header).await,
			KeygenRound::Next => dkg_worker.get_next_best_authorities(header).await,
		};

		let best_authorities = best_authorities
			.into_iter()
			.flat_map(|(i, p)| KeygenPartyId::try_from(i).map(|i| (i, p)))
			.collect();

		let authority_public_key = dkg_worker.get_authority_public_key();
		let proto_stage_ty = if stage == KeygenRound::Genesis {
			ProtoStageType::KeygenGenesis
		} else {
			ProtoStageType::KeygenStandard
		};

		dkg_worker.logger.debug(format!("🕸️  PARTY {party_i} | SPAWNING KEYGEN SESSION {session_id} | BEST AUTHORITIES: {best_authorities:?}"));

		let keygen_protocol_hash = get_keygen_protocol_hash(
			session_id,
			self.active_keygen_retry_id.load(Ordering::SeqCst),
		);

		// If we are starting this keygen because of an emergency keygen, clear the unsigned
		// proposals locally
		if anticipated_execution_status.force_execute {
			dkg_worker.signing_manager.clear_enqueued_proposal_tasks();
		}

		// For now, always use the MpEcdsa variant
		let params = KeygenProtocolSetupParameters::MpEcdsa {
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			associated_block: *header.number(),
			threshold,
			stage: proto_stage_ty,
			keygen_protocol_hash,
		};

		let dkg = dkg_worker
			.dkg_modules
			.get_keygen_protocol(&params)
			.expect("Default should be present");

		if let Some((handle, task)) = dkg.initialize_keygen_protocol(params).await {
			// Before sending the task, force clear all previous tasks to allow the new one
			// the immediately run
			if anticipated_execution_status.force_execute {
				dkg_worker.logger.debug(
					"🕸️  PARTY {party_i} | SPAWNING KEYGEN SESSION {session_id} | FORCE EXECUTE",
				);
				self.work_manager.force_shutdown_all();
			}

			if let Err(err) = self.push_task(handle, task) {
				dkg_worker.logger.error(format!(
					"🕸️  PARTY {party_i} | SPAWNING KEYGEN SESSION {session_id} | ERROR: {err}"
				));

				dkg_worker.handle_dkg_error(err).await;
			} else {
				// update states
				match stage {
					KeygenRound::Genesis => self.set_state(KeygenState::RunningGenesisKeygen),
					KeygenRound::Next => self.set_state(KeygenState::RunningKeygen),
				}

				self.latest_executed_session_id.store(Some(session_id), Ordering::Relaxed);
			}
		}
	}

	/// Pushes a task to the work manager, manually polling and starting the keygen protocol
	pub fn push_task(
		&self,
		handle: AsyncProtocolRemote<NumberFor<B>>,
		task: Pin<Box<dyn SendFuture<'static, ()>>>,
	) -> Result<(), DKGError> {
		let task_hash = get_keygen_protocol_hash(
			handle.session_id,
			self.active_keygen_retry_id.load(Ordering::Relaxed),
		);
		self.work_manager.push_task(task_hash, false, handle, task)?;
		// poll to start the task
		self.work_manager.poll();
		Ok(())
	}
}

/// Computes keccak_256(session ID || retry_id)
fn get_keygen_protocol_hash(session_id: u64, active_keygen_retry_id: usize) -> [u8; 32] {
	let mut session_id_bytes = session_id.to_be_bytes().to_vec();
	let retry_id_bytes = active_keygen_retry_id.to_be_bytes();
	session_id_bytes.extend_from_slice(&retry_id_bytes);
	keccak_256(&session_id_bytes)
}
