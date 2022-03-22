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
use sp_core::ecdsa;
pub use sp_core::sr25519;
use sp_io::{hashing::keccak_256, EcdsaVerifyError};
use sp_runtime::traits::BadOrigin;
use sp_std::vec::Vec;

use crate::traits::GetDKGPublicKey;

pub const SIGNATURE_LENGTH: usize = 65;
const KEY_LENGTH: usize = 32;

pub fn validate_ecdsa_signature(data: &[u8], signature: &[u8]) -> bool {
	if signature.len() == SIGNATURE_LENGTH {
		let mut sig = [0u8; SIGNATURE_LENGTH];
		sig[..SIGNATURE_LENGTH].copy_from_slice(signature);

		let hash = keccak_256(data);

		sp_io::crypto::secp256k1_ecdsa_recover(&sig, &hash).is_ok()
	} else {
		false
	}
}

pub fn recover_ecdsa_pub_key(data: &[u8], signature: &[u8]) -> Result<Vec<u8>, EcdsaVerifyError> {
	if signature.len() == SIGNATURE_LENGTH {
		let mut sig = [0u8; SIGNATURE_LENGTH];
		sig[..SIGNATURE_LENGTH].copy_from_slice(signature);

		let hash = keccak_256(data);

		let pub_key = sp_io::crypto::secp256k1_ecdsa_recover(&sig, &hash)?;
		return Ok(pub_key.to_vec())
	}
	Err(EcdsaVerifyError::BadSignature)
}

pub fn verify_signer_from_set_ecdsa(
	maybe_signers: Vec<ecdsa::Public>,
	msg: &[u8],
	signature: &[u8],
) -> (Option<ecdsa::Public>, bool) {
	let mut signer = None;
	let res = maybe_signers.iter().any(|x| {
		if let Ok(data) = recover_ecdsa_pub_key(msg, signature) {
			if x.0.to_vec() == data {
				signer = Some(x.clone());
				true
			} else {
				false
			}
		} else {
			false
		}
	});

	(signer, res)
}

pub fn verify_signer_from_set(
	maybe_signers: Vec<sr25519::Public>,
	msg: &[u8],
	signature: &[u8],
) -> (Option<sr25519::Public>, bool) {
	let mut signer = None;
	let res = maybe_signers.iter().any(|x| {
		let decoded_signature = sr25519::Signature::from_slice(signature);
		let res = sp_io::crypto::sr25519_verify(&decoded_signature, msg, x);
		if res {
			signer = Some(*x);
		}

		res
	});
	(signer, res)
}

pub fn to_slice_32(val: &[u8]) -> Option<[u8; 32]> {
	if val.len() == KEY_LENGTH {
		let mut key = [0u8; KEY_LENGTH];
		key[..KEY_LENGTH].copy_from_slice(val);

		return Some(key)
	}

	None
}

/// This function takes the ecdsa signature and the unhashed data
pub fn ensure_signed_by_dkg<T: GetDKGPublicKey>(
	signature: &[u8],
	data: &[u8],
) -> Result<(), BadOrigin> {
	let dkg_key = T::dkg_key();
	frame_support::log::debug!(
		target: "dkg",
		"Stored public key: {:?}",
		dkg_key
	);
	if dkg_key.len() != 33 {
		return Err(BadOrigin)
	}

	let recovered_key = recover_ecdsa_pub_key(data, signature);

	match recovered_key {
		Ok(recovered_pub_key) => {
			let current_dkg = &dkg_key[1..];
			// The stored_key public key is 33 bytes long and contains the prefix which is the first
			// byte.
			// The recovered key does not contain the prefix and is 64 bytes long, we take a
			// slice of the first 32 bytes because the dkg_key is a compressed public key.
			frame_support::log::debug!(
				target: "dkg",
				"Recovered: \npublic key: {:?}\ncurr_key: {:?}",
				recovered_pub_key, current_dkg
			);
			let signer = &recovered_pub_key[..32];
			// now we do check if the signer is not the current dkg or the previous one.
			let is_not_current_dkg = signer != current_dkg;
			if is_not_current_dkg {
				let prev_key = T::previous_dkg_key();
				if prev_key.len() != 33 {
					return Err(BadOrigin)
				}
				let prev_dkg = &prev_key[1..];
				let is_not_prev_dkg = signer != prev_dkg;
				if is_not_prev_dkg {
					return Err(BadOrigin)
				}
			}
		},
		Err(_) => return Err(BadOrigin),
	}

	Ok(())
}
