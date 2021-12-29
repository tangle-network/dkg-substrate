use crate::{
	rounds::{CompletedOfflineStage, LocalKey},
	types::RoundId,
};
use bincode::serialize;
use chacha20poly1305::{
	aead::{Aead, NewAead},
	XChaCha20Poly1305,
};
use codec::Encode;
use sc_service::{ChainType, Configuration};
use serde::{Deserialize, Serialize};
use sp_core::{sr25519, Pair, Public};
use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};
use sp_runtime::key_types::ACCOUNT;
use std::{fs, path::PathBuf};

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

pub fn insert_controller_account_keys_into_keystore(
	config: &Configuration,
	key_store: Option<SyncCryptoStorePtr>,
) {
	let chain_type = config.chain_spec.chain_type();
	let seed = &config.network.node_name[..];

	match seed {
		// When running the chain in dev or local test net, we insert the sr25519 account keys for collator accounts or validator accounts into the keystore
		// Only if the node running is one of the predefined nodes Alice, Bob, Charlie, Dave, Eve or Ferdie
		"Alice" | "Bob" | "Charlie" | "Dave" | "Eve" | "Ferdie" => {
			if chain_type == ChainType::Development || chain_type == ChainType::Local {
				let pub_key = get_from_seed::<sr25519::Public>(&seed).encode();
				if let Some(keystore) = key_store {
					let _ = SyncCryptoStore::insert_unknown(
						&*keystore,
						ACCOUNT,
						&format!("//{}", seed),
						&pub_key,
					);
				}
			}
		},
		_ => {},
	}
}

pub const DKG_OFFLINE_STAGE_FILE: &str = "dkg_completed_offline_stage";
pub const DKG_LOCAL_KEY_FILE: &str = "dkg_local_key";
pub const QUEUED_DKG_OFFLINE_STAGE_FILE: &str = "queued_dkg_completed_offline_stage";
pub const QUEUED_DKG_LOCAL_KEY_FILE: &str = "queued_dkg_local_key";

#[derive(Deserialize, Serialize)]
pub struct StoredLocalKey {
	pub round_id: RoundId,
	pub local_key: LocalKey,
}

#[derive(Serialize, Deserialize)]
pub struct StoredOfflineStage {
	pub round_id: RoundId,
	pub completed_offlinestage: CompletedOfflineStage,
}

// TODO: Encrypt data before storing

pub fn store_localkey(key: LocalKey, round_id: RoundId, path: PathBuf) -> std::io::Result<()> {
	let stored_local_key = StoredLocalKey { round_id, local_key: key };

	let serialized_data = serialize(&stored_local_key);

	if let Ok(data) = serialized_data {
		fs::write(path, data)?;
		return Ok(())
	}
	Err(std::io::ErrorKind::Other.into())
}

pub fn store_offline_stage(
	offline_stage: CompletedOfflineStage,
	round_id: RoundId,
	path: PathBuf,
) -> std::io::Result<()> {
	let stored_local_key = StoredOfflineStage { round_id, completed_offlinestage: offline_stage };

	let serialized_data = serialize(&stored_local_key);

	if let Ok(data) = serialized_data {
		fs::write(path, data)?;
		return Ok(())
	}

	Err(std::io::ErrorKind::Other.into())
}

pub fn cleanup(path: PathBuf) -> std::io::Result<()> {
	fs::remove_file(path)?;
	Ok(())
}
// The secret key bytes should be the byte representation of the secret field of an srr25519 keypair
pub fn encrypt_data(data: Vec<u8>, secret_key_bytes: Vec<u8>) -> Result<Vec<u8>, &'static str> {
	if secret_key_bytes.len() != 64 {
		return Err("Secret key bytes must be 64bytes long")
	}
	let key = &secret_key_bytes[..32];
	// Nonce is 32 bytes, we only need the first 24bytes for the encryption algorithm
	let nonce = &secret_key_bytes[32..][..24];
	let cipher = XChaCha20Poly1305::new(key.into());

	let encrypted_data =
		cipher.encrypt(nonce.into(), &data[..]).map_err(|_| "File encryption failed")?;
	Ok(encrypted_data)
}

pub fn decrypt_data(data: Vec<u8>, secret_key_bytes: Vec<u8>) -> Result<Vec<u8>, &'static str> {
	if secret_key_bytes.len() != 64 {
		return Err("Secret key bytes must be 64bytes long")
	}

	let key = &secret_key_bytes[..32];
	let nonce = &secret_key_bytes[32..][..24];
	let cipher = XChaCha20Poly1305::new(key.into());
	let decrypted_data =
		cipher.decrypt(nonce.into(), &data[..]).map_err(|_| "File decryption failed")?;
	Ok(decrypted_data)
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_keyring::AccountKeyring::Alice;

	fn encrypt(data: Vec<u8>) -> Vec<u8> {
		let pair = Alice.pair();
		let key_pair = pair.as_ref();
		let secret_key = key_pair.secret.to_bytes();

		let encrypted_data = encrypt_data(data, secret_key.to_vec());
		assert!(encrypted_data.is_ok());

		encrypted_data.unwrap()
	}

	fn decrypt(data: Vec<u8>) -> Vec<u8> {
		let pair = Alice.pair();
		let key_pair = pair.as_ref();
		let secret_key = key_pair.secret.to_bytes();
		let decrypted_data = decrypt_data(data, secret_key.to_vec());
		assert!(decrypted_data.is_ok());

		decrypted_data.unwrap()
	}

	#[test]
	fn should_encrypt_and_decrypt_data() {
		let data = b"Hello world";

		let encrypted_data = encrypt(data.to_vec());
		let decrypted_data = decrypt(encrypted_data.clone());
		println!("{:?}", encrypted_data);
		assert!(encrypted_data != data.to_vec());

		println!("{:?}, {:?}", data, decrypted_data);
		assert!(decrypted_data == data.to_vec());
	}
}
