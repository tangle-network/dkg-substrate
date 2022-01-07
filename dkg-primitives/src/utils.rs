use crate::{rounds::LocalKey, types::RoundId};
use bincode::serialize;
use chacha20poly1305::{
	aead::{Aead, NewAead},
	XChaCha20Poly1305,
};
use codec::Encode;
use curv::elliptic::curves::Secp256k1;
use sc_service::{ChainType, Configuration};
use serde::{Deserialize, Serialize};
use sp_core::{sr25519, Pair, Public};
use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};
use sp_runtime::key_types::ACCOUNT;
use std::{
	fs,
	io::{Error, ErrorKind},
	path::PathBuf,
};

use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};

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

pub fn vec_usize_to_u16(input: Vec<usize>) -> Vec<u16> {
	return input.iter().map(|v| *v as u16).collect()
}

pub const DKG_LOCAL_KEY_FILE: &str = "dkg_local_key";
pub const QUEUED_DKG_LOCAL_KEY_FILE: &str = "queued_dkg_local_key";

#[derive(Deserialize, Serialize, Clone)]
pub struct StoredLocalKey {
	pub round_id: RoundId,
	pub local_key: LocalKey<Secp256k1>,
}

pub fn store_localkey(
	key: LocalKey<Secp256k1>,
	round_id: RoundId,
	path: PathBuf,
	secret_key: Vec<u8>,
) -> std::io::Result<()> {
	let stored_local_key = StoredLocalKey { round_id, local_key: key };

	let serialized_data = serialize(&stored_local_key)
		.map_err(|_| Error::new(ErrorKind::Other, "Serialization failed"))?;

	let encrypted_data =
		encrypt_data(serialized_data, secret_key).map_err(|e| Error::new(ErrorKind::Other, e))?;
	fs::write(path, &encrypted_data[..])?;
	Ok(())
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
	// The Nonce is 32 bytes, we only need 24 bytes for the encryption algorithm
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

pub fn select_random_set(
	seed: &[u8],
	set: Vec<u16>,
	amount: u16,
) -> Result<Vec<u16>, &'static str> {
	if seed.len() != 32 {
		return Err("Seed must be 32 bytes")
	}
	let mut slice = [0u8; 32];
	slice.copy_from_slice(seed);
	let mut std_rng = <StdRng as SeedableRng>::from_seed(slice);
	let random_set =
		set.choose_multiple(&mut std_rng, amount as usize).cloned().collect::<Vec<_>>();
	Ok(random_set)
}

#[cfg(test)]
mod tests {
	use super::*;
	use rand::RngCore;
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

	#[test]
	fn should_generate_same_random_set_for_the_same_seed() {
		let mut rng = rand::thread_rng();
		let mut seed = [0u8; 32];
		rng.fill_bytes(&mut seed);

		let set = (1..=16).collect::<Vec<u16>>();
		let mut random_set = Vec::new();
		for _ in 0..100 {
			let new_set = select_random_set(&seed, set.clone(), 10).unwrap();
			if random_set.len() > 0 {
				assert!(random_set == new_set);
			}
			random_set = new_set;
		}
	}
}
