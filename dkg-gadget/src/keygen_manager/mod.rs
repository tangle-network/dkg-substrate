use crate::{
	async_protocols::{remote::AsyncProtocolRemote, KeygenPartyId, KeygenRound},
	gossip_engine::GossipEngineIface,
	signing_manager::work_manager::{JobMetadata, PollMethod, WorkManager},
	utils::SendFuture,
	worker::{DKGWorker, HasLatestHeader, KeystoreExt, ProtoStageType},
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

pub struct KeygenManager<B: Block, BE, C, GE> {
	// governs the workload for each node
	work_manager: WorkManager<B>,
	active_keygen_retry_id: Arc<AtomicUsize>,
	keygen_state: Arc<Atomic<KeygenState>>,
	latest_executed_session_id: Arc<Atomic<Option<SessionId>>>,
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
		}
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeygenState {
	Uninitialized,
	RunningKeygen,
	RunningGenesisKeygen,
	GenesisKeygenCompleted,
	// session_completed denotes the session that executed the keygen, NOT
	// the generated DKG public key for the next session
	KeygenCompleted { session_completed: u64 },
}

// only 1 task at a time may run for keygen
const MAX_RUNNING_TASKS: usize = 1;

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
				PollMethod::Manual,
			),
			active_keygen_retry_id: Arc::new(AtomicUsize::new(0)),
			keygen_state: Arc::new(Atomic::new(KeygenState::Uninitialized)),
			latest_executed_session_id: Arc::new(Atomic::new(None)),
			_pd: Default::default(),
		}
	}

	pub fn deliver_message(&self, message: SignedDKGMessage<Public>) {
		self.work_manager.deliver_message(message)
	}

	pub fn session_id_of_active_keygen(&self, now: NumberFor<B>) -> Option<JobMetadata> {
		self.work_manager.get_active_session_ids(now).pop()
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

	pub async fn on_block_finalized(
		&self,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) {
		let now_n = *header.number();
		let now: u64 = now_n.saturated_into();
		let current_protocol = self.session_id_of_active_keygen(now_n);
		let state = self.state();
		dkg_worker.logger.debug(format!(
			"*** KeygenManager on_block_finalized: now={now}, state={state:?}, current_protocol={current_protocol:?}",
		));

		/*
		   Session 0 (beginning): immediately run genesis
		   Session 0 (ending): run keygen for session 1
		   Session 1 (ending): run keygen for session 2
		   Session 2 (ending): run keygen for session 3
		   Session 3 (ending): run keygen for session 4
		*/

		/*
		   Genesis logic
		*/

		if now == GENESIS_AUTHORITY_SET_ID && state == KeygenState::Uninitialized {
			// if we are at genesis, and there is no active keygen, create and immediately start()
			// one
			return self
				.maybe_start_keygen_for_stage(KeygenRound::Genesis, header, dkg_worker)
				.await
		}

		if now == GENESIS_AUTHORITY_SET_ID && state == KeygenState::RunningGenesisKeygen {
			// if we are at genesis, and genesis keygen is running, do nothing
			return
		}

		if now == GENESIS_AUTHORITY_SET_ID && state == KeygenState::GenesisKeygenCompleted {
			return self
				.maybe_start_keygen_for_stage(KeygenRound::GenesisNext, header, dkg_worker)
				.await
		}

		if now == GENESIS_AUTHORITY_SET_ID && state == KeygenState::RunningKeygen {
			// if we are at genesis, and there is an active keygen, do nothing
			return
		}

		if now == GENESIS_AUTHORITY_SET_ID &&
			matches!(state, KeygenState::KeygenCompleted { session_completed: 0 })
		{
			// if we are at genesis, and we have completed session-0-ending keygen, do nothing;
			// cleanup will be handled by the job manager
			return
		}

		/*
		   Non-Genesis logic
		*/

		// check bad states. These should never happen in a well-behaved program
		if now > GENESIS_AUTHORITY_SET_ID && state == KeygenState::GenesisKeygenCompleted ||
			state == KeygenState::RunningGenesisKeygen
		{
			dkg_worker.logger.error(format!("Invalid keygen manager state: {now} > GENESIS_AUTHORITY_SET_ID && {state:?} == KeygenState::GenesisKeygenCompleted || {state:?} == KeygenState::RunningGenesisKeygen"));
			return
		}

		if now > GENESIS_AUTHORITY_SET_ID && state == KeygenState::Uninitialized {
			// we joined the network after genesis. We need to start a keygen for session `now`,
			// so long as the next pub key isn't already on chain
			if dkg_worker.get_next_dkg_pub_key(header).await.is_none() {
				return self
					.maybe_start_keygen_for_stage(KeygenRound::Next, header, dkg_worker)
					.await
			}
		}

		if now > GENESIS_AUTHORITY_SET_ID && state == KeygenState::RunningKeygen {
			// we are in the middle of a keygen. Do nothing
			return
		}

		if now > GENESIS_AUTHORITY_SET_ID && matches!(state, KeygenState::KeygenCompleted { .. }) {
			if let KeygenState::KeygenCompleted { session_completed } = state {
				if session_completed + 1 == now {
					// we need to start a keygen for session `now`:
					return self
						.maybe_start_keygen_for_stage(KeygenRound::Next, header, dkg_worker)
						.await
				}

				if session_completed == now {
					dkg_worker.logger.info("We are complete with the current session's keygen. The job manager will handle cleanup")
				}
			} else {
				unreachable!("We already checked this case above")
			}
		}
	}

	async fn maybe_start_keygen_for_stage(
		&self,
		stage: KeygenRound,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) {
		if stage != KeygenRound::Genesis {
			// we need to ensure session progress is close enough to the end to begin execution
			if !dkg_worker.should_execute_new_keygen(header).await {
				dkg_worker.logger.debug("🕸️  Not executing new keygen protocol");
				return
			}
		}
		// DKG keygen authorities are always taken from the best set of authorities
		if let Some((authority_set, _queued)) = dkg_worker.validator_set(header).await {
			let session_id = authority_set.id;
			// Check whether the worker is in the best set or return
			let party_i = match dkg_worker.get_party_index(header).await {
				Some(party_index) => {
					dkg_worker.logger.info(format!("🕸️  PARTY {party_index} | SESSION {session_id} | IN THE SET OF BEST GENESIS AUTHORITIES: session: {session_id}"));
					if let Ok(res) = KeygenPartyId::try_from(party_index) {
						res
					} else {
						return
					}
				},
				None => {
					dkg_worker.logger.info(format!(
						"🕸️  NOT IN THE SET OF BEST GENESIS AUTHORITIES: session: {session_id}"
					));
					return
				},
			};

			let best_authorities = dkg_worker
				.get_best_authorities(header)
				.await
				.into_iter()
				.flat_map(|(i, p)| KeygenPartyId::try_from(i).map(|i| (i, p)))
				.collect();
			let threshold = dkg_worker.get_signature_threshold(header).await;
			let authority_public_key = dkg_worker.get_authority_public_key();
			let proto_stage_ty = if stage == KeygenRound::Genesis {
				ProtoStageType::KeygenGenesis
			} else {
				ProtoStageType::KeygenStandard
			};

			dkg_worker.logger.debug(format!("🕸️  PARTY {party_i} | SPAWNING KEYGEN SESSION {session_id} | BEST AUTHORITIES: {best_authorities:?}"));

			if let Some((handle, task)) = dkg_worker
				.initialize_keygen_protocol(
					best_authorities,
					authority_public_key,
					party_i,
					session_id,
					*header.number(),
					threshold,
					proto_stage_ty,
				)
				.await
			{
				if let Err(err) = self.push_task(handle, task) {
					dkg_worker.logger.error(format!(
						"🕸️  PARTY {party_i} | SPAWNING KEYGEN SESSION {session_id} | ERROR: {err}"
					));

					dkg_worker.handle_dkg_error(err).await;
				} else {
					// update states
					match stage {
						KeygenRound::Genesis => self.set_state(KeygenState::RunningGenesisKeygen),

						KeygenRound::GenesisNext | KeygenRound::Next =>
							self.set_state(KeygenState::RunningKeygen),
					}

					self.latest_executed_session_id.store(Some(session_id), Ordering::Relaxed);
				}
			}
		} else {
			dkg_worker.logger.warn("Could not obtain validator set")
		}
	}

	pub fn push_task(
		&self,
		handle: AsyncProtocolRemote<NumberFor<B>>,
		task: Pin<Box<dyn SendFuture<'static, ()>>>,
	) -> Result<(), DKGError> {
		// keccak_256(compute session ID || retry_id)
		let mut session_id_bytes = handle.session_id.to_be_bytes().to_vec();
		let retry_id_bytes = self
			.active_keygen_retry_id
			.load(std::sync::atomic::Ordering::Relaxed)
			.to_be_bytes();
		session_id_bytes.extend_from_slice(&retry_id_bytes);
		let task_hash = keccak_256(&session_id_bytes);
		self.work_manager.push_task(task_hash, handle, task)?;
		// poll to start the task
		self.work_manager.poll();
		Ok(())
	}
}
