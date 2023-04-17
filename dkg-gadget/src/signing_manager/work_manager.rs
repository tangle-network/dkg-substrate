use std::{sync::Arc, collections::HashSet};
use dkg_primitives::types::DKGError;
use parking_lot::RwLock;
use sp_api::BlockT;
use crate::{debug_logger::DebugLogger, async_protocols::remote::AsyncProtocolRemote};
use std::hash::{Hash, Hasher};
use crate::NumberFor;
use std::future::Future;
use std::pin::Pin;

#[derive(Clone)]
pub struct WorkManager<B: BlockT> {
    pub currently_signing_proposals: Arc<RwLock<HashSet<Job<B>>>>,
    pub enqueued_signing_proposals: Arc<RwLock<HashSet<Job<B>>>>,
    // for now, use a hard-coded value for the number of tasks
    max_tasks: usize,
    logger: DebugLogger,
    to_handler: tokio::sync::mpsc::UnboundedSender<[u8; 32]>
}

impl<B: BlockT> WorkManager<B> {
    pub fn new(logger: DebugLogger, max_tasks: usize) -> Self {
        let (to_handler, rx) = tokio::sync::mpsc::unbounded_channel();
        let this = Self {
            currently_signing_proposals: Arc::new(RwLock::new(HashSet::new())),
            enqueued_signing_proposals: Arc::new(RwLock::new(HashSet::new())),
            max_tasks,
            logger,
            to_handler
        };

        let this_worker = this.clone();
        let handler = async move {
            let job_receiver = async move {
                while let Some(unsigned_proposal_hash) = rx.recv().await {
                    this_worker.logger.info_signing(format!("[worker] Received job {:?}", unsigned_proposal_hash));
                    this_worker.poll(Some(unsigned_proposal_hash));
                }
            };

            let periodic_poller = async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
                loop {
                    interval.tick().await;
                    this_worker.poll(None);
                }
            };

            tokio::select! {
                _ = job_receiver => {
                    this_worker.logger.error_signing("[worker] job_receiver exited");
                },
                _ = periodic_poller => {
                    this_worker.logger.error_signing("[worker] periodic_poller exited");
                }
            }
        };

        tokio::task::spawn(handler);

        this
    }

    /// Pushes the task, but does not necessarily start it
    pub fn push_task(&self, unsigned_proposal_hash: [u8; 32], handle: AsyncProtocolRemote<NumberFor<B>>, task: Pin<Box<dyn Future<Output = Result<(), DKGError>> + Send + 'static>>) -> Result<(), DKGError> {
        let mut lock = self.enqueued_signing_proposals.write();
        let job = Job { handle, proposal_hash: Arc::new(unsigned_proposal_hash), currently_signing_proposals: self.currently_signing_proposals.clone(), logger: self.logger.clone() };
        if lock.insert(job) {
            self.logger.warn_signing(format!("Overwrote existing job {:?}", unsigned_proposal_hash));
        }

        self.to_handler.send(unsigned_proposal_hash).map_err(|_| DKGError::GenericError { reason: "Failed to send job to worker".to_string() })
    }

    fn poll(&self, new_task: Option<[u8; 32]>) {
        if let Some(task) = new_task {
            self.maybe_start_new_task(task);
        } else {
            // go through each task and see if it's done
            // finally, see if we can start a new task
        }
    }

    fn maybe_start_new_task(&self, task: [u8; 32]) {
        let mut lock = self.currently_signing_proposals.write();
        if lock.len() < self.max_tasks {
            let mut enqueued_lock = self.enqueued_signing_proposals.write();
            if let Some(job) = enqueued_lock.take(&task) {
                if let Err(err) = job.handle.start() {
                    self.logger.error_signing(format!("Failed to start job {task:?}: {err:?}"));
                }
                lock.insert(job);
            }
        }
    }
}

#[derive(Clone)]
pub struct Job<B: BlockT> {
    // wrap in an arc to get the strong count for this job
    proposal_hash: Arc<[u8; 32]>,
    currently_signing_proposals: Arc<RwLock<HashSet<Job<B>>>>,
    logger: DebugLogger,
    handle: AsyncProtocolRemote<NumberFor<B>>
}

impl<B: BlockT> std::borrow::Borrow<[u8; 32]> for Job<B> {
    fn borrow(&self) -> &[u8; 32] {
        &self.proposal_hash
    }
}

impl<B: BlockT> PartialEq for Job<B> {
    fn eq(&self, other: &Self) -> bool {
        self.proposal_hash == other.proposal_hash
    }
}

impl<B: BlockT> Eq for Job<B> {}

impl<B: BlockT> Hash for Job<B> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.proposal_hash.hash(state);
    }
}

impl<B: BlockT> Drop for Job<B> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.proposal_hash) == 1 {
            self.logger.info_signing(format!("Will remove job {:?} from currently_signing_proposals", self.proposal_hash));
            self.currently_signing_proposals.write().remove(self);
            let _ = self.handle.shutdown("shutdown from Job::drop");
        }
    }
}