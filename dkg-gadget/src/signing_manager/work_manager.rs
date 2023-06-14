use crate::{
	async_protocols::remote::AsyncProtocolRemote, debug_logger::DebugLogger, utils::SendFuture,
	worker::HasLatestHeader, NumberFor,
};
use dkg_primitives::{
	crypto::Public,
	types::{DKGError, SignedDKGMessage},
};
use dkg_runtime_primitives::associated_block_id_acceptable;
use parking_lot::RwLock;
use sp_api::BlockT;
use std::{
	collections::{HashMap, HashSet, VecDeque},
	hash::{Hash, Hasher},
	pin::Pin,
	sync::Arc,
};
use sync_wrapper::SyncWrapper;

// How often to poll the jobs to check completion status
const JOB_POLL_INTERVAL_IN_MILLISECONDS: u64 = 500;

#[derive(Clone)]
pub struct WorkManager<B: BlockT> {
	inner: Arc<RwLock<WorkManagerInner<B>>>,
	clock: Arc<dyn HasLatestHeader<B>>,
	// for now, use a hard-coded value for the number of tasks
	max_tasks: usize,
	logger: DebugLogger,
	to_handler: tokio::sync::mpsc::UnboundedSender<[u8; 32]>,
}

pub struct WorkManagerInner<B: BlockT> {
	pub active_tasks: HashSet<Job<B>>,
	pub enqueued_tasks: VecDeque<Job<B>>,
	pub enqueued_messages: HashMap<[u8; 32], VecDeque<SignedDKGMessage<Public>>>,
}

impl<B: BlockT> WorkManager<B> {
	pub fn new(logger: DebugLogger, clock: impl HasLatestHeader<B>, max_tasks: usize) -> Self {
		let (to_handler, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let this = Self {
			inner: Arc::new(RwLock::new(WorkManagerInner {
				active_tasks: HashSet::new(),
				enqueued_tasks: VecDeque::new(),
				enqueued_messages: HashMap::new(),
			})),
			clock: Arc::new(clock),
			max_tasks,
			logger,
			to_handler,
		};

		let this_worker = this.clone();
		let handler = async move {
			let job_receiver_worker = this_worker.clone();
			let logger = job_receiver_worker.logger.clone();

			let job_receiver = async move {
				while let Some(task_hash) = rx.recv().await {
					job_receiver_worker
						.logger
						.info_signing(format!("[worker] Received job {task_hash:?}",));
					job_receiver_worker.poll();
				}
			};

			let periodic_poller = async move {
				let mut interval = tokio::time::interval(std::time::Duration::from_millis(
					JOB_POLL_INTERVAL_IN_MILLISECONDS,
				));
				loop {
					interval.tick().await;
					this_worker.poll();
				}
			};

			tokio::select! {
				_ = job_receiver => {
					logger.error_signing("[worker] job_receiver exited");
				},
				_ = periodic_poller => {
					logger.error_signing("[worker] periodic_poller exited");
				}
			}
		};

		tokio::task::spawn(handler);

		this
	}

	/// Pushes the task, but does not necessarily start it
	pub fn push_task(
		&self,
		task_hash: [u8; 32],
		mut handle: AsyncProtocolRemote<NumberFor<B>>,
		task: Pin<Box<dyn SendFuture<'static, ()>>>,
	) -> Result<(), DKGError> {
		let mut lock = self.inner.write();
		// set as primary, that way on drop, the async protocol ends
		handle.set_as_primary();
		let job = Job {
			task: Arc::new(RwLock::new(Some(task.into()))),
			handle,
			task_hash,
			logger: self.logger.clone(),
		};
		lock.enqueued_tasks.push_back(job);

		self.to_handler.send(task_hash).map_err(|_| DKGError::GenericError {
			reason: "Failed to send job to worker".to_string(),
		})
	}

	fn poll(&self) {
		// go through each task and see if it's done
		// finally, see if we can start a new task
		let now = self.clock.get_latest_block_number();
		let mut lock = self.inner.write();
		let cur_count = lock.active_tasks.len();
		lock.active_tasks.retain(|job| {
			let is_stalled = job.handle.signing_has_stalled(now);
			if is_stalled {
				// if stalled, lets log the start and now blocks for logging purposes
				self.logger.info_signing(format!(
					"[worker] Job {:?} | Started at {:?} | Now {:?} | is stalled, shutting down",
					hex::encode(job.task_hash),
					job.handle.started_at,
					now
				));

				// the task is stalled, lets be pedantic and shutdown
				let _ = job.handle.shutdown("Stalled!");
				// return false so that the proposals are released from the currently signing
				// proposals
				return false
			}

			let is_done = job.handle.is_done();
			/*self.logger.info_signing(format!(
				"[worker] Job {:?} is done: {}",
				hex::encode(job.task_hash),
				is_done
			));*/

			!is_done
		});

		let new_count = lock.active_tasks.len();
		if cur_count != new_count {
			self.logger
				.info_signing(format!("[worker] {} jobs dropped", cur_count - new_count));
		}

		// now, check to see if there is room to start a new task
		let tasks_to_start = self.max_tasks - lock.active_tasks.len();
		for _ in 0..tasks_to_start {
			if let Some(job) = lock.enqueued_tasks.pop_front() {
				self.logger.info_signing(format!(
					"[worker] Starting job {:?}",
					hex::encode(job.task_hash)
				));
				if let Err(err) = job.handle.start() {
					self.logger.error_signing(format!(
						"Failed to start job {:?}: {err:?}",
						hex::encode(job.task_hash)
					));
				} else {
					// deliver all the enqueued messages to the protocol now
					if let Some(mut enqueued_messages) =
						lock.enqueued_messages.remove(&job.task_hash)
					{
						self.logger.info_signing(format!(
							"Will now deliver {} enqueued message(s) to the async protocol for {:?}",
							enqueued_messages.len(),
							hex::encode(job.task_hash)
						));
						while let Some(message) = enqueued_messages.pop_front() {
							if let Err(err) = job.handle.deliver_message(message) {
								self.logger.error_signing(format!(
									"Unable to deliver message for job {:?}: {err:?}",
									hex::encode(job.task_hash)
								));
							}
						}
					}
				}
				let task = job.task.clone();
				// Put the job inside here, that way the drop code does not get called right away,
				// killing the process
				lock.active_tasks.insert(job);
				// run the task
				let task = async move {
					let task = task.write().take().expect("Should not happen");
					task.into_inner().await
				};

				// Spawn the task. When it finishes, it will clean itself up
				tokio::task::spawn(task);
			}
		}
	}

	pub fn job_exists(&self, job: &[u8; 32]) -> bool {
		let lock = self.inner.read();
		lock.active_tasks.contains(job) || lock.enqueued_tasks.iter().any(|j| &j.task_hash == job)
	}

	pub fn deliver_message(&self, msg: SignedDKGMessage<Public>) {
		self.logger.debug_signing(format!(
			"Delivered message is intended for session_id = {}",
			msg.msg.session_id
		));
		let mut lock = self.inner.write();

		let msg_unsigned_proposal_hash =
			msg.msg.payload.unsigned_proposal_hash().expect("Bad message type");

		// check the enqueued
		for task in lock.enqueued_tasks.iter() {
			if task.handle.session_id == msg.msg.session_id &&
				&task.task_hash == msg_unsigned_proposal_hash &&
				associated_block_id_acceptable(
					task.handle.associated_block_id,
					msg.msg.associated_block_id,
				) {
				self.logger.debug(format!(
					"Message is for this ENQUEUED signing execution in session: {}",
					task.handle.session_id
				));
				if let Err(_err) = task.handle.deliver_message(msg) {
					self.logger.warn_signing("Failed to deliver message to signing task");
				}

				return
			}
		}

		// check the currently signing
		for task in lock.active_tasks.iter() {
			if task.handle.session_id == msg.msg.session_id &&
				&task.task_hash == msg_unsigned_proposal_hash &&
				associated_block_id_acceptable(
					task.handle.associated_block_id,
					msg.msg.associated_block_id,
				) {
				self.logger.debug(format!(
					"Message is for this signing CURRENT execution in session: {}",
					task.handle.session_id
				));
				if let Err(_err) = task.handle.deliver_message(msg) {
					self.logger.warn_signing("Failed to deliver message to signing task");
				}

				return
			}
		}

		// if the protocol is neither started nor enqueued, then, this message may be for a future
		// async protocol. Store the message
		self.logger.info_signing(format!(
			"Enqueuing message for {:?}",
			hex::encode(msg_unsigned_proposal_hash)
		));
		lock.enqueued_messages
			.entry(*msg_unsigned_proposal_hash)
			.or_default()
			.push_back(msg)
	}
}

pub struct Job<B: BlockT> {
	// wrap in an arc to get the strong count for this job
	task_hash: [u8; 32],
	logger: DebugLogger,
	handle: AsyncProtocolRemote<NumberFor<B>>,
	task: Arc<RwLock<Option<SyncFuture<()>>>>,
}

pub type SyncFuture<T> = SyncWrapper<Pin<Box<dyn SendFuture<'static, T>>>>;

impl<B: BlockT> std::borrow::Borrow<[u8; 32]> for Job<B> {
	fn borrow(&self) -> &[u8; 32] {
		&self.task_hash
	}
}

impl<B: BlockT> PartialEq for Job<B> {
	fn eq(&self, other: &Self) -> bool {
		self.task_hash == other.task_hash
	}
}

impl<B: BlockT> Eq for Job<B> {}

impl<B: BlockT> Hash for Job<B> {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.task_hash.hash(state);
	}
}

impl<B: BlockT> Drop for Job<B> {
	fn drop(&mut self) {
		self.logger.info_signing(format!(
			"Will remove job {:?} from currently_signing_proposals",
			hex::encode(self.task_hash)
		));
		let _ = self.handle.shutdown("shutdown from Job::drop");
	}
}
