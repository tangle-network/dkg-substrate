pub use sp_core::sr25519;
use sp_io::{hashing::keccak_256, EcdsaVerifyError};
use sp_std::vec::Vec;

pub const SIGNATURE_LENGTH: usize = 65;
const KEY_LENGTH: usize = 32;

pub fn validate_ecdsa_signature(data: &Vec<u8>, signature: &Vec<u8>) -> bool {
	if signature.len() == SIGNATURE_LENGTH {
		let mut sig = [0u8; SIGNATURE_LENGTH];
		sig[..SIGNATURE_LENGTH].copy_from_slice(&signature);

		let hash = keccak_256(&data);

		return sp_io::crypto::secp256k1_ecdsa_recover(&sig, &hash).is_ok()
	} else {
		return false
	}
}

pub fn recover_ecdsa_pub_key(
	data: &Vec<u8>,
	signature: &Vec<u8>,
) -> Result<Vec<u8>, EcdsaVerifyError> {
	if signature.len() == SIGNATURE_LENGTH {
		let mut sig = [0u8; SIGNATURE_LENGTH];
		sig[..SIGNATURE_LENGTH].copy_from_slice(&signature);

		let hash = keccak_256(&data);

		let pub_key = sp_io::crypto::secp256k1_ecdsa_recover(&sig, &hash)?;
		return Ok(pub_key.to_vec())
	}
	Err(EcdsaVerifyError::BadSignature)
}

pub fn verify_signer_from_set(
	maybe_signers: Vec<sr25519::Public>,
	msg: &Vec<u8>,
	signature: &Vec<u8>,
) -> (Option<sr25519::Public>, bool) {
	let mut signer = None;
	let res = maybe_signers.iter().any(|x| {
		let decoded_signature = sr25519::Signature::from_slice(&signature[..]);
		let res = sp_io::crypto::sr25519_verify(&decoded_signature, &msg[..], x);
		if res {
			signer = Some(x.clone());
		}

		res
	});
	(signer, res)
}

pub fn to_slice_32(val: &Vec<u8>) -> Option<[u8; 32]> {
	if val.len() == KEY_LENGTH {
		let mut key = [0u8; KEY_LENGTH];
		key[..KEY_LENGTH].copy_from_slice(&val);

		return Some(key)
	}

	return None
}
