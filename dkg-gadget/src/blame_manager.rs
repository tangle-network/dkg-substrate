use dkg_logging::debug_logger::DebugLogger;
use dkg_runtime_primitives::SessionId;
use itertools::Itertools;
use parking_lot::Mutex;
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

#[derive(Clone, Debug)]
pub struct BlameManager {
	inner: Arc<Mutex<BlameManagerInner>>,
}

#[derive(Debug)]
struct BlameManagerInner {
	keygen_blame: HashMap<SessionId, HashSet<u16>>,
	signing_blame: HashMap<SessionId, HashSet<u16>>,
	debug_logger: DebugLogger,
}

impl BlameManager {
	pub fn new(debug_logger: DebugLogger) -> Self {
		Self {
			inner: Arc::new(Mutex::new(BlameManagerInner {
				keygen_blame: HashMap::new(),
				signing_blame: HashMap::new(),
				debug_logger,
			})),
		}
	}

	/// Replace any blamed nodes inside `input` with non-blamed nodes inside `all_parties`
	pub fn filter_signing_set(
		&self,
		session_id: SessionId,
		input: &Vec<u16>,
		all_parties: &Vec<u16>,
	) -> Option<Vec<u16>> {
		let required_length = input.len();
		let mut lock = self.inner.lock();
		let mut result = HashSet::new();

		// First, store any non-blamed nodes from `input` into `result`
		for party in input {
			if !lock.signing_blame.entry(session_id).or_default().contains(party) {
				lock.debug_logger.debug(format!("[BlameManager] Adding non-blamed node {party} from input signing set into output signing set"));
				result.insert(*party);
			}
		}

		// If `result` is still not long enough, add non-blamed nodes from `all_parties`
		if result.len() < required_length {
			for party in all_parties {
				if !lock.signing_blame.entry(session_id).or_default().contains(party) {
					lock.debug_logger.debug(format!("[BlameManager] Adding non-blamed node {party} from all_parties set into output signing set"));
					result.insert(*party);
				}

				if result.len() == required_length {
					break
				}
			}
		}

		// If `result` is still not long enough, add blamed nodes from `input`
		if result.len() < required_length {
			lock.debug_logger.warn(
				"[BlameManager] Not enough parties to construct a valid signing set".to_string(),
			);
			return None
		}

		Some(result.into_iter().sorted().collect())
	}
	/// Deletes all sessions previous to the given session id.
	pub fn on_session_rotated(&self, new_session: SessionId) {
		let mut lock = self.inner.lock();
		lock.keygen_blame.retain(|session_id, _| *session_id >= new_session);
		lock.signing_blame.retain(|session_id, _| *session_id >= new_session);
	}

	pub fn update_blame(&self, session_id: SessionId, blame: &[u16], keygen: bool) {
		let mut lock = self.inner.lock();
		if keygen {
			lock.keygen_blame.entry(session_id).or_default().extend(blame);
		} else {
			lock.signing_blame.entry(session_id).or_default().extend(blame);
		}
	}

	pub fn signing_session_has_blame(&self, session_id: SessionId) -> bool {
		let mut lock = self.inner.lock();
		!lock.signing_blame.entry(session_id).or_default().is_empty()
	}

	#[cfg(test)]
	fn clear_blame(&self) {
		let mut lock = self.inner.lock();
		lock.keygen_blame.clear();
		lock.signing_blame.clear();
	}
}

#[cfg(test)]
mod tests {
	#[tokio::test]
	// Test that the filter_signing_set function works as expected
	async fn test_blame_filter() {
		use super::*;
		use dkg_logging::debug_logger::DebugLogger;

		dkg_logging::setup_log();
		let blame_manager = BlameManager::new(DebugLogger::new("test", None).unwrap());
		let session_id = 0;
		let all_parties = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

		// Test 1: No blamed nodes, input set = len 3
		let input = vec![0, 1, 2];
		let expected = vec![0, 1, 2];
		assert_eq!(
			blame_manager.filter_signing_set(session_id, &input, &all_parties).unwrap(),
			expected
		);

		// Test 2: No blamed nodes, input set = len 2
		let input = vec![0, 1];
		let expected = vec![0, 1];
		assert_eq!(
			blame_manager.filter_signing_set(session_id, &input, &all_parties).unwrap(),
			expected
		);

		// Test 3: Some blamed nodes, input set = len 3
		let input = vec![0, 1, 2];
		blame_manager.update_blame(session_id, &[0, 1], false);
		let expected = vec![2, 3, 4];
		assert_eq!(
			blame_manager.filter_signing_set(session_id, &input, &all_parties).unwrap(),
			expected
		);
		blame_manager.clear_blame();

		// Test 4: More blamed nodes
		let input = vec![0, 1, 2];
		blame_manager.update_blame(session_id, &[0, 1, 2], false);
		let expected = vec![3, 4, 5];
		assert_eq!(
			blame_manager.filter_signing_set(session_id, &input, &all_parties).unwrap(),
			expected
		);
		blame_manager.clear_blame();

		// Test 5: Most all blamed nodes
		let input = vec![0, 1, 2];
		blame_manager.update_blame(session_id, &[0, 2, 3, 5, 6, 7, 8], false); // < -- 1,4, and 9 not blamed
		let expected = vec![1, 4, 9];
		assert_eq!(
			blame_manager.filter_signing_set(session_id, &input, &all_parties).unwrap(),
			expected
		);
		blame_manager.clear_blame();

		// Test 6: Not enough non-blamed nodes
		let input = vec![0, 1, 2];
		blame_manager.update_blame(session_id, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9], false);
		assert_eq!(blame_manager.filter_signing_set(session_id, &input, &all_parties), None);
		blame_manager.clear_blame();
	}
}
