use std::{sync::{Arc, atomic::AtomicUsize}, collections::HashSet};
use parking_lot::RwLock;
use crate::debug_logger::DebugLogger;

#[derive(Clone)]
pub struct WorkManager {
    currently_signing_proposals: Arc<RwLock<HashSet<Job>>>,
    // for now, use a hard-coded value for the number of tasks
    max_tasks: usize,
    debug_logger: DebugLogger
}

#[derive(Clone, Debug)]
pub struct Job {
    // wrap in an arc to get the strong count for this job
    proposal_hash: Arc<[u8; 32]>,
    currently_signing_proposals: Arc<RwLock<HashSet<Job>>>,
    logger: DebugLogger,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.proposal_hash == other.proposal_hash
    }
}

impl Eq for Job {}

impl Hash for Job {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.proposal_hash.hash(state);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if Arc::strong_count(this.proposal_hash) == 1 {
            self.debug_logger.info_signing("Will remove job {:?} from currently_signing_proposals", self.proposal_hash);
            self.currently_signing_proposals.write().remove(self);
        }
    }
}