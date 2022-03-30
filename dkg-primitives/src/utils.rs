// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use crate::{rounds::LocalKey, types::RoundId};
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
use std::{collections::HashMap, fs, hash::Hash, path::PathBuf};

use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

/// Inserts a key of type `ACCOUNT` into the keystore for development/testing.
///
/// Currently, this only successfully inserts keys if the seed is development related.
/// i.e. for Alice, Bob, Charlie, etc.
pub fn insert_controller_account_keys_into_keystore(
	config: &Configuration,
	key_store: Option<SyncCryptoStorePtr>,
) {
	let chain_type = config.chain_spec.chain_type();
	let seed = &config.network.node_name[..];

	match seed {
		// When running the chain in dev or local test net, we insert the sr25519 account keys for
		// collator accounts or validator accounts into the keystore Only if the node running is one
		// of the predefined nodes Alice, Bob, Charlie, Dave, Eve or Ferdie
		"Alice" | "Bob" | "Charlie" | "Dave" | "Eve" | "Ferdie" => {
			if chain_type == ChainType::Development || chain_type == ChainType::Local {
				let pub_key = get_from_seed::<sr25519::Public>(seed).encode();
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

pub fn cleanup(path: PathBuf) -> std::io::Result<()> {
	fs::remove_file(path)?;
	Ok(())
}

/// Encrypt a vector of bytes `data` with a `secret_key`.
///
/// The secret key bytes should be the byte representation of the secret field of an sr25519 keypair
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

/// Decrypt an encrypted `data` with a `secret_key_bytes` using
/// XChaCha20Poly1305 cipher.
pub fn decrypt_data(data: Vec<u8>, secret_key_bytes: Vec<u8>) -> Result<Vec<u8>, &'static str> {
	if secret_key_bytes.len() != 64 {
		return Err("Secret key bytes must be 64-bytes long")
	}

	let key = &secret_key_bytes[..32];
	let nonce = &secret_key_bytes[32..][..24];
	let cipher = XChaCha20Poly1305::new(key.into());
	let decrypted_data =
		cipher.decrypt(nonce.into(), &data[..]).map_err(|_| "File decryption failed")?;
	Ok(decrypted_data)
}

/// Select a random subset of unsigned u16 from a vector of u16s
/// of size `amount` with a random seed of `seed`.
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

/// Selects a subset of authorities of size `count` based
/// on signed integer reputations for each authority.
///
/// If there is no reputation for an authority, it defaults to
/// a reputation of 0. The sort applied to the authorities by
/// reputation is expected to be stable, so that the same set
/// of authorities is returned even in the case of equivalent reputations.
pub fn get_best_authorities<B>(
	count: usize,
	authorities: &[B],
	reputations: &HashMap<B, i64>,
) -> Vec<(u16, B)>
where
	B: Eq + Hash + Clone,
{
	let mut reputations_of_authorities = authorities
		.iter()
		.enumerate()
		.map(|(index, id)| (index + 1, reputations.get(id).unwrap_or(&0), id))
		.collect::<Vec<(_, _, _)>>();
	reputations_of_authorities.sort_by(|a, b| b.1.cmp(a.1));

	return reputations_of_authorities
		.iter()
		.map(|x| (x.0 as u16, x.2.clone()))
		.take(count)
		.collect()
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
			if !random_set.is_empty() {
				assert!(random_set == new_set);
			}
			random_set = new_set;
		}
	}
}
