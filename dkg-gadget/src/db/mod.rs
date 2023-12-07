use dkg_primitives::{types::DKGError, SessionId};

mod mem;
mod offchain_storage;

use crate::async_protocols::types::LocalKeyType;
pub use mem::DKGInMemoryDb;
pub use offchain_storage::DKGOffchainStorageDb;

/// A Database backend, specifically for the DKG to store and load important state
///
/// The backend of this database could be using a persistence store or in-memory
/// ephemeral store, depending on the use case. For example, during the tests we can switch
/// to an in-memory store, and in production we could use sled database or Offchain storage.
#[auto_impl::auto_impl(Arc, &, &mut)]
pub trait DKGDbBackend: Send + Sync + 'static {
	/// Returns the DKG [`LocalKey<Secp256k1>`] at specific session, if any.
	fn get_local_key(&self, session_id: SessionId) -> Result<Option<LocalKeyType>, DKGError>;
	/// Stores the [`LocalKey<Secp256k1>`] at a specified session.
	fn store_local_key(
		&self,
		session_id: SessionId,
		local_key: LocalKeyType,
	) -> Result<(), DKGError>;
}
