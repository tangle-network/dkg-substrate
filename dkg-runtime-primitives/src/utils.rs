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
pub use sp_core::{ecdsa, sr25519};
use sp_io::{hashing::keccak_256, EcdsaVerifyError};
use sp_runtime::traits::BadOrigin;
use sp_std::vec::Vec;

use crate::traits::GetDKGPublicKey;

pub const SIGNATURE_LENGTH: usize = 65;
const SR25519_KEY_LENGTH: usize = 32;
const ECDSA_KEY_LENGTH: usize = 33;

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
			let recovered = &data[..32];
			if x.0[1..].to_vec() == recovered.to_vec() {
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
) -> (Option<sr25519::Public>, bool, Option<usize>) {
	let mut signer = None;
	let mut inx: Option<usize> = None;
	let res =
		maybe_signers.iter().enumerate().any(|(index, x)| {
			match sr25519::Signature::from_slice(signature) {
				Some(decoded_signature) => {
					let res = sp_io::crypto::sr25519_verify(&decoded_signature, msg, x);
					if res {
						inx = Some(index);
						signer = Some(*x);
					}

					res
				},
				None => false,
			}
		});
	(signer, res, inx)
}

pub fn to_slice_32(val: &[u8]) -> Option<[u8; 32]> {
	if val.len() == SR25519_KEY_LENGTH {
		let mut key = [0u8; SR25519_KEY_LENGTH];
		key[..SR25519_KEY_LENGTH].copy_from_slice(val);

		return Some(key)
	}

	None
}

pub fn to_slice_33(val: &[u8]) -> Option<[u8; 33]> {
	if val.len() == ECDSA_KEY_LENGTH {
		let mut key = [0u8; ECDSA_KEY_LENGTH];
		key[..ECDSA_KEY_LENGTH].copy_from_slice(val);

		return Some(key)
	}

	None
}

pub struct SignatureResult {
	pub expected: Vec<u8>,
	pub actual: Vec<u8>,
}

pub enum SignatureError {
	InvalidDKGKey(BadOrigin),
	InvalidRecovery(SignatureResult),
	InvalidECDSASignature(BadOrigin),
}

/// This function takes the ecdsa signature and the unhashed data
pub fn ensure_signed_by_dkg<T: GetDKGPublicKey>(
	signature: &[u8],
	data: &[u8],
) -> Result<(), SignatureError> {
	let dkg_key = T::dkg_key();

	let recovered_key = recover_ecdsa_pub_key(data, signature)
		.map_err(|_| SignatureError::InvalidECDSASignature(BadOrigin))?;

	// Validate possibility of using current key
	if dkg_key.is_empty() || dkg_key.len() != 33 {
		return Err(SignatureError::InvalidDKGKey(BadOrigin))
	}

	let current_dkg = &dkg_key[1..];

	#[cfg(feature = "std")]
	frame_support::log::debug!(
		target: "dkg",
		"Recovered:
		**********************************************************
		public key: {}
		curr_key: {}
		**********************************************************",
		hex::encode(recovered_key.clone()),
		hex::encode(current_dkg),
	);

	// The stored_key public key is 33 bytes compressed.
	// The recovered key is 64 bytes uncompressed. The first 32 bytes represent the compressed
	// portion of the key.
	let signer = &recovered_key[..32];
	// Check if the signer is the current DKG or the previous DKG to buffer for timing issues
	let is_not_current_dkg = signer != current_dkg;

	if !is_not_current_dkg {
		Ok(())
	} else {
		Err(SignatureError::InvalidRecovery(SignatureResult {
			expected: dkg_key.to_vec(),
			actual: recovered_key.clone(),
		}))
	}
}
