//! In Memory storage backend for DKG database, this is used for testing purposes.
//! the data is stored in memory and is not persisted, and is lost when the process is killed.

use std::{collections::BTreeMap, sync::Mutex};

use crate::async_protocols::types::LocalKeyType;
use dkg_primitives::{types::DKGError, SessionId};

type LockedMap<K, V> = Mutex<BTreeMap<K, V>>;

/// In Memory storage backend for DKG database, this is used for testing purposes.
pub struct DKGInMemoryDb {
	local_keys: LockedMap<SessionId, LocalKeyType>,
}

impl Default for DKGInMemoryDb {
	fn default() -> Self {
		Self::new()
	}
}

impl DKGInMemoryDb {
	/// Create a new instance of [`DKGInMemoryDb`].
	#[allow(unused)]
	pub fn new() -> Self {
		Self { local_keys: Mutex::new(BTreeMap::new()) }
	}
}

impl super::DKGDbBackend for DKGInMemoryDb {
	fn get_local_key(&self, session_id: SessionId) -> Result<Option<LocalKeyType>, DKGError> {
		let lock = self.local_keys.lock().map_err(|e| DKGError::CriticalError {
			reason: format!("Failed to lock local_keys: {e}"),
		})?;
		Ok(lock.get(&session_id).cloned())
	}

	fn store_local_key(
		&self,
		session_id: SessionId,
		local_key: LocalKeyType,
	) -> Result<(), DKGError> {
		let mut lock = self.local_keys.lock().map_err(|e| DKGError::CriticalError {
			reason: format!("Failed to lock local_keys: {e}"),
		})?;
		lock.insert(session_id, local_key);
		Ok(())
	}
}
