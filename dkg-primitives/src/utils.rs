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
use chacha20poly1305::{
	aead::{Aead, NewAead},
	XChaCha20Poly1305,
};
use codec::Encode;
use curv::arithmetic::Converter;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::SignatureRecid;

use sc_service::{ChainType, Configuration};
use sp_core::{sr25519, Pair, Public};
use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};
use sp_runtime::key_types::ACCOUNT;

use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use sp_core::ecdsa::Signature;

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{seed}"), None)
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
						&format!("//{seed}"),
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
		println!("{encrypted_data:?}");
		assert_ne!(encrypted_data, data.to_vec());

		println!("{data:?}, {decrypted_data:?}");
		assert_eq!(decrypted_data, data.to_vec());
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
				assert_eq!(random_set, new_set);
			}
			random_set = new_set;
		}
	}
}

pub fn convert_signature(sig_recid: &SignatureRecid) -> Option<Signature> {
	let r = sig_recid.r.to_bigint().to_bytes();
	let s = sig_recid.s.to_bigint().to_bytes();
	let v = sig_recid.recid + 27u8;

	let mut sig_vec: Vec<u8> = Vec::new();

	for _ in 0..(32 - r.len()) {
		sig_vec.extend([0]);
	}
	sig_vec.extend_from_slice(&r);

	for _ in 0..(32 - s.len()) {
		sig_vec.extend([0]);
	}
	sig_vec.extend_from_slice(&s);

	sig_vec.extend([v]);

	if 65 != sig_vec.len() {
		log::warn!(target: "dkg", "🕸️  Invalid signature len: {}, expected 65", sig_vec.len());
		return None
	}

	let mut dkg_sig_arr: [u8; 65] = [0; 65];
	dkg_sig_arr.copy_from_slice(&sig_vec[0..65]);

	Some(Signature(dkg_sig_arr))
}
