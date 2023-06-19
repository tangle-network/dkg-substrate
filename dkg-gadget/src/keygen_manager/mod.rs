use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use sc_client_api::Backend;
use sp_arithmetic::traits::SaturatedConversion;
use sp_runtime::traits::{Block, Header, NumberFor};
use dkg_logging::debug_logger::DebugLogger;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::{AuthorityId, Public};
use dkg_runtime_primitives::{DKGApi, GENESIS_AUTHORITY_SET_ID, keccak_256, MaxAuthorities, MaxProposalLength, SessionId};
use crate::async_protocols::remote::AsyncProtocolRemote;
use crate::Client;
use crate::gossip_engine::GossipEngineIface;
use crate::signing_manager::work_manager::{JobMetadata, PollMethod, WorkManager};
use crate::utils::SendFuture;
use crate::worker::{DKGWorker, HasLatestHeader};

pub struct KeygenManager<B: Block, BE, C, GE> {
	// governs the workload for each node
	work_manager: WorkManager<B>,
	active_keygen_retry_id: Arc<AtomicUsize>,
	_pd: PhantomData<(B, BE, C, GE)>,
}

impl<B: Block, BE, C, GE> Clone for KeygenManager<B, BE, C, GE> {
	fn clone(&self) -> Self {
		Self { work_manager: self.work_manager.clone(), _pd: self._pd, active_keygen_retry_id: self.active_keygen_retry_id.clone() }
	}
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
			work_manager: WorkManager::<B>::new(logger, clock, MAX_RUNNING_TASKS, PollMethod::Manual),
			active_keygen_retry_id: Arc::new(AtomicUsize::new(0)),
			_pd: Default::default(),
		}
	}

	pub fn deliver_message(&self, message: SignedDKGMessage<Public>) {
		self.work_manager.deliver_message(message)
	}

	pub fn session_id_of_active_keygen(&self, now: NumberFor<B>) -> Option<JobMetadata> {
		self.work_manager.get_active_session_ids(now).pop()
	}

	pub fn session_id_of_enqueued_keygen(&self, now: NumberFor<B>) -> Option<JobMetadata> {
		self.work_manager.get_enqueued_session_ids(now).pop()
	}

	pub fn on_block_finalized(&self, header: &B::Header, dkg_worker: &DKGWorker<B, BE, C, GE>,) {
		let now_n = header.number().clone();
		let now: u64 = header.number().saturated_into();
		let current_protocol = self.session_id_of_active_keygen(now_n);

		if now == GENESIS_AUTHORITY_SET_ID && current_protocol.is_none() {
			// if we are at genesis, and there is no active keygen, create and immediately start() one
			// TODO: start genesis keygen
			return
		}

		if now > GENESIS_AUTHORITY_SET_ID {
			// in a well-behaved program, this should always be Some
			if let Some(current_protocol) = current_protocol {
				if !current_protocol.has_started {
					// the finality notification we received is for a non-genesis block, and, our current protocol has not started.
					// since we didn't decide to start it earlier (i.e., a non-genesis keygen), we must start it here IF
					// the session ID has incremented
					if current_protocol.session_id == now {
						// polling will automatically start the task for us
						self.work_manager.poll();
					}
				}

				if current_protocol.is_stalled {
					// the finality notification we received is for a non-genesis block, and, our current protocol is stalled.
					// we must increment the reset counter, and, start a new task
					// TODO
					return
				}

				if current_protocol.is_finished {
					// the finality notification we received is for a non-genesis block, and, our current protocol is finished.
					// we must reset the retry counter, and, create a current with session ID + 1. We only start the task if we are now in a new session
					// TODO
					return
				}
			} else {
				dkg_worker.logger.error("No active keygen, and we are not at genesis. This should not happen.");
			}
		}
	}

	pub fn push_task(&self, handle: AsyncProtocolRemote<NumberFor<B>>, task: Pin<Box<dyn SendFuture<'static, ()>>>) -> Result<(), DKGError> {
		// keccak_256(compute session ID || retry_id)
		let mut session_id_bytes = handle.session_id.to_be_bytes().to_vec();
		let retry_id_bytes = self.active_keygen_retry_id.load(std::sync::atomic::Ordering::Relaxed).to_be_bytes();
		session_id_bytes.extend_from_slice(&retry_id_bytes);
		let task_hash = keccak_256(&session_id_bytes);
		self.work_manager.push_task(task_hash, handle, task)?;
		Ok(())
	}
}
