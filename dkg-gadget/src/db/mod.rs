use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{types::DKGError, SessionId};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;

mod mem;
mod offchain_storage;

pub use mem::DKGInMemoryDb;
// pub use offchain_storage::DKGOffchainStorageDb;

/// A Database backend, specificly for the DKG to store and load important state
///
/// The backend of this database could be using a persistence store, or in-memory
/// ephermal store, depending on the use case, for example during the tests we can switch
/// to in-memory store, and in production we could use sled database or Offchain storage.
#[auto_impl::auto_impl(Arc, &, &mut)]
pub trait DKGDbBackend: Send + Sync + 'static {
	/// Returns the DKG [`LocalKey<Secp256k1>`] at specific session, if any.
	fn get_local_key(&self, session_id: SessionId)
		-> Result<Option<LocalKey<Secp256k1>>, DKGError>;
	/// Stores the [`LocalKey<Secp256k1>`] at this session.
	fn store_local_key(
		&self,
		session_id: SessionId,
		local_key: LocalKey<Secp256k1>,
	) -> Result<(), DKGError>;
}
