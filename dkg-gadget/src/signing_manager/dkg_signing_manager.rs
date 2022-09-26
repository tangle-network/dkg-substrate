use std::sync::Arc;
use std::collections::VecDeque;

use dkg_primitives::UnsignedProposal;
use parking_lot::RwLock;
use tokio::sync::broadcast::Sender;

pub struct DKGSigningManager {
	message_queue: MessageToSignQueue<UnsignedProposal>,
}

pub struct DKGSigningManagerController {
	to_worker: Sender<Command>,
}

enum Command {
	SignMessage(UnsignedProposal),
	Reset,
}

pub struct DKGSigningManagerBuilder;

#[derive(Clone, Debug, Eq)]
struct DKGSigingSet(Vec<u16>);

impl PartialEq for DKGSigingSet {
    fn eq(&self, other: &Self) -> bool {
		let mut a = self.0.clone();
		let mut b = other.0.clone();
		a.sort_unstable();
		b.sort_unstable();
        a == b
    }
}

impl std::hash::Hash for DKGSigingSet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		let mut s = self.0.clone();
		s.sort_unstable();
        s.hash(state);
    }
}

#[derive(Clone)]
struct MessageToSignQueue<M> {
	inner: Arc<RwLock<VecDeque<M>>>,
}

impl<M> MessageToSignQueue<M> {
	pub fn new() -> Self {
		Self {
			inner: Arc::new(RwLock::new(VecDeque::with_capacity(256))),
		}
	}

	pub fn enqueue(&self, msg: M) {
		self.inner.write().push_back(msg);
	}

	pub fn dequeue(&self) -> Option<M> {
		self.inner.write().pop_front()
	}

	pub fn peek(&self) -> Option<M> where M: Clone {
		self.inner.read().front().cloned()
	}

	pub fn is_empty(&self) -> bool {
		self.inner.read().is_empty()
	}

	pub fn len(&self) -> usize {
		self.inner.read().len()
	}
}

#[cfg(test)]
mod tests {
    use super::*;
	use std::hash::{Hash, Hasher};

    #[test]
    fn same_sets() {
        let s1 = DKGSigingSet(vec![1, 2, 3]);
		let s2 = DKGSigingSet(vec![2, 3, 1]);
		assert_eq!(s1, s2);
		let mut hasher = std::collections::hash_map::DefaultHasher::new();
		s1.hash(&mut hasher);
		let h1 = hasher.finish();
		let mut hasher = std::collections::hash_map::DefaultHasher::new();
		s2.hash(&mut hasher);
		let h2 = hasher.finish();
		assert_eq!(h1, h2);
    }
}
