//! DKG Database backend, implemented using Offchain Storage.
//! Unlike the in-memory database, this database is persistent and can be used to store
//! the DKG state across multiple runs of the node.

use std::sync::Arc;

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	types::DKGError,
	utils::{decrypt_data, encrypt_data},
	SessionId,
};
use dkg_runtime_primitives::offchain::crypto::{Pair as AppPair, Public};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_core::{offchain::OffchainStorage, Pair};
use sp_runtime::traits::Block;

use crate::DKGKeystore;

/// DKG Offchain Storage prefix.
const STORAGE_PREFIX: &[u8] = b"dkg";

/// A Database backend, specifically for the DKG to store and load important state
/// implemented using Offchain Storage.
///
/// This backend also uses the DKG Keystore to store the data in an encrypted form.
pub struct DKGOffchainStorageDb<B, BE> {
	backend: Arc<BE>,
	key_store: DKGKeystore,
	local_keystore: Option<Arc<LocalKeystore>>,
	__marker: std::marker::PhantomData<B>,
}

impl<B, BE> DKGOffchainStorageDb<B, BE> {
	pub fn new(
		backend: Arc<BE>,
		dkg_key_store: DKGKeystore,
		local_keystore: Option<Arc<LocalKeystore>>,
	) -> Self {
		Self { backend, key_store: dkg_key_store, local_keystore, __marker: Default::default() }
	}
}

/// A submodule to hold the database keys.
mod keys {
	use super::*;
	#[derive(Debug, Clone, codec::Encode, codec::Decode)]
	pub(super) struct LocalKey {
		/// "dkg" letters.
		_prefix: [u8; 3],
		/// Key name "local_key".
		_key_name: [u8; 9],
		/// Session ID.
		pub session_id: SessionId,
	}

	impl LocalKey {
		pub fn new(session_id: SessionId) -> Self {
			Self { _prefix: *b"dkg", _key_name: *b"local_key", session_id }
		}
	}
}

impl<B, BE> super::DKGDbBackend for DKGOffchainStorageDb<B, BE>
where
	B: Block,
	BE: Backend<B> + 'static,
{
	fn get_local_key(
		&self,
		session_id: SessionId,
	) -> Result<Option<LocalKey<Secp256k1>>, DKGError> {
		dkg_logging::info!(target: "dkg", "Offchain Storage : Fetching local keys for session {:?}", session_id);
		let db_key = keys::LocalKey::new(session_id);
		let maybe_decrypted_bytes = self.load_and_decrypt(codec::Encode::encode(&db_key))?;
		match maybe_decrypted_bytes {
			Some(decrypted_bytes) => {
				let local_key = serde_json::from_slice(&decrypted_bytes.0)
					.map_err(|e| DKGError::CriticalError { reason: e.to_string() })?;
				dkg_logging::info!(target: "dkg", "Offchain Storage : Fetched local keys for session {:?}, Key : {:?}", session_id, local_key);
				Ok(Some(local_key))
			},
			None => Ok(None),
		}
	}

	fn store_local_key(
		&self,
		session_id: SessionId,
		local_key: LocalKey<Secp256k1>,
	) -> Result<(), DKGError> {
		dkg_logging::info!(target: "dkg", "Offchain Storage : Store local keys for session {:?}, Key : {:?}", session_id, local_key);
		let db_key = keys::LocalKey::new(session_id);
		let value = serde_json::to_vec(&local_key)
			.map_err(|e| DKGError::CriticalError { reason: e.to_string() })?;
		self.encrypt_and_store(codec::Encode::encode(&db_key), value)
	}
}
// ** These are wrapper types to make a typesafe difference between the encrypted and raw data.
// ** This is to prevent accidental misuse of the data.
struct EncryptedBytes(Vec<u8>);
impl EncryptedBytes {
	fn new(bytes: Vec<u8>) -> Self {
		Self(bytes)
	}
}

struct DecryptedBytes(Vec<u8>);
impl DecryptedBytes {
	fn new(bytes: Vec<u8>) -> Self {
		Self(bytes)
	}
}

impl<B, BE> DKGOffchainStorageDb<B, BE>
where
	B: Block,
	BE: Backend<B>,
{
	/// Fetch the secret key from the keystore and use it to encrypt and decrypt the data.
	///
	/// This needs at least one sr25519 key in the keystore.
	fn secret_key(&self) -> Result<Vec<u8>, DKGError> {
		let public_key = self
			.key_store
			.sr25519_public_key(&self.key_store.sr25519_public_keys().unwrap_or_default())
			.ok_or_else(|| DKGError::CriticalError {
				reason: String::from("No sr25519 Keys in the Keystore!!"),
			})?;
		let local_keystore = match &self.local_keystore {
			Some(keystore) => keystore,
			None =>
				return Err(DKGError::CriticalError { reason: String::from("No Local Keystore!!") }),
		};
		let our_public_key =
			Public::try_from(&*public_key).map_err(|_| DKGError::CriticalError {
				reason: String::from("Failed to convert Public Key to sp_core::offchain::Public"),
			})?;
		let key_pair = local_keystore.key_pair::<AppPair>(&our_public_key).map_err(|e| {
			DKGError::CriticalError {
				reason: format!("Error getting key pair from local keystore: {e}"),
			}
		})?;
		match key_pair {
			Some(pair) => Ok(pair.to_raw_vec()),
			None => Err(DKGError::CriticalError {
				reason: String::from("No Key Pair in the Local Keystore!!"),
			}),
		}
	}

	/// Encrypts the raw data and stores it in the offchain storage.
	///
	/// Note: This will overwrite any existing data at the given key.
	fn encrypt_and_store(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), DKGError> {
		let secret_key = self.secret_key()?;
		let encrypted_data = encrypt_data(value, secret_key).map_err(|e| {
			DKGError::CriticalError { reason: format!("Error encrypting data: {e}") }
		})?;
		self.store_encrypted_bytes(key, EncryptedBytes::new(encrypted_data))
	}

	/// Loads the encrypted data from the offchain storage and decrypts it.
	///
	/// Returns None if the key is not found.
	/// Returns an error if the decryption fails.
	fn load_and_decrypt(&self, key: Vec<u8>) -> Result<Option<DecryptedBytes>, DKGError> {
		let secret_key = self.secret_key()?;
		let maybe_encrypted_data = self.load_encrypted_bytes(key)?;
		match maybe_encrypted_data {
			Some(encrypted_data) => {
				let decrypted_data = decrypt_data(encrypted_data.0, secret_key).map_err(|e| {
					DKGError::CriticalError { reason: format!("Error decrypting data: {e}") }
				})?;
				Ok(Some(DecryptedBytes::new(decrypted_data)))
			},
			None => Ok(None),
		}
	}

	/// Stores the encrypted bytes in the offchain storage.
	/// You should use [`Self::encrypt_and_store`] for a more convenient method.
	///
	/// Note: This will overwrite any existing data at the given key.
	fn store_encrypted_bytes(&self, key: Vec<u8>, value: EncryptedBytes) -> Result<(), DKGError> {
		self.store(key, value.0)
	}

	/// Loads the encrypted bytes from the offchain storage.
	/// You should use [`Self::load_and_decrypt`] for a more convenient method.
	///
	/// Returns None if the key is not found.
	/// Returns an error if the decryption fails.
	fn load_encrypted_bytes(&self, key: Vec<u8>) -> Result<Option<EncryptedBytes>, DKGError> {
		let maybe_bytes = self.load(key)?;
		match maybe_bytes {
			Some(bytes) => Ok(Some(EncryptedBytes::new(bytes))),
			None => Ok(None),
		}
	}

	/// Stores the raw bytes in the offchain storage.
	///
	/// Note: This will overwrite any existing data at the given key.
	/// Note: The stored data is not encrypted, so use this with care.
	fn store(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), DKGError> {
		let mut offchain_storage = self.backend.offchain_storage().ok_or_else(|| {
			DKGError::CriticalError { reason: String::from("No Offchain Storage available!!") }
		})?;
		if offchain_storage.get(STORAGE_PREFIX, &key).is_some() {
			dkg_logging::warn!(
				"Offchain Storage : Overwriting already existing database entry at key 0x{}",
				hex::encode(key.clone())
			);
		}
		offchain_storage.set(STORAGE_PREFIX, &key, &value);
		Ok(())
	}

	/// Loads the raw bytes from the offchain storage.
	///
	/// Returns None if the key is not found.
	/// Note: The loaded data may not be encrypted, so use this with care.
	fn load(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, DKGError> {
		let offchain_storage = self.backend.offchain_storage().ok_or_else(|| {
			DKGError::CriticalError { reason: String::from("No Offchain Storage available!!") }
		})?;
		Ok(offchain_storage.get(STORAGE_PREFIX, &key))
	}
}
