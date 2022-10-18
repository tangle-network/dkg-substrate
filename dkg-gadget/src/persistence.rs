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

use crate::async_protocols::remote::MetaHandlerStatus;
use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	serde_json,
	types::SessionId,
	utils::{decrypt_data, encrypt_data, StoredLocalKey},
};
use dkg_runtime_primitives::offchain::crypto::{Pair as AppPair, Public};
use log::debug;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use sc_keystore::LocalKeystore;
use serde::{Deserialize, Serialize};
use sp_core::Pair;
use sp_runtime::traits::{AtLeast32BitUnsigned, Block, NumberFor};
use std::{
	fs,
	io::{Error, ErrorKind},
	path::PathBuf,
	sync::Arc,
};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StoredRoundsMetadata<C: AtLeast32BitUnsigned + Copy> {
	pub session_id: SessionId,
	pub status: MetaHandlerStatus,
	pub started_at: C,
}

pub(crate) fn store_saved_rounds<B>(
	session_id: SessionId,
	started_at: NumberFor<B>,
	status: MetaHandlerStatus,
	base_path: Option<PathBuf>,
) -> std::io::Result<()>
where
	B: Block,
{
	if let Some(path) = base_path {
		let stored_rounds_metadata = StoredRoundsMetadata { session_id, started_at, status };
		let serialized_data = serde_json::to_string(&stored_rounds_metadata)
			.map_err(|_| Error::new(ErrorKind::Other, "Serialization failed"))?;
		fs::write(path, &serialized_data[..])?;

		Ok(())
	} else {
		Err(Error::new(ErrorKind::Other, "Base path not found, need to handle default path"))
	}
}

pub(crate) fn load_saved_rounds<B>(
	base_path: Option<PathBuf>,
) -> std::io::Result<StoredRoundsMetadata<NumberFor<B>>>
where
	B: Block,
{
	if let Some(path) = base_path {
		let serialized_data = fs::read(path)?;
		let stored_rounds_metdata: StoredRoundsMetadata<NumberFor<B>> =
			serde_json::from_slice(&serialized_data)
				.map_err(|_| Error::new(ErrorKind::Other, "Deserialization failed"))?;
		Ok(stored_rounds_metdata)
	} else {
		Err(Error::new(ErrorKind::Other, "Base path not found, need to handle default path"))
	}
}

pub(crate) fn store_localkey(
	key: LocalKey<Secp256k1>,
	session_id: SessionId,
	path: Option<PathBuf>,
	local_keystore: Option<&Arc<LocalKeystore>>,
	sr25519_public_key: sp_core::sr25519::Public,
) -> std::io::Result<()> {
	if let Some(path) = path {
		if let Some(local_keystore) = local_keystore {
			debug!(target: "dkg_persistence", "Storing local key for {:?}", &path);
			let key_pair = local_keystore.key_pair::<AppPair>(
				&Public::try_from(&sr25519_public_key.0[..])
					.unwrap_or_else(|_| panic!("Could not find keypair in local key store")),
			);
			if let Ok(Some(key_pair)) = key_pair {
				let secret_key = key_pair.to_raw_vec();

				let stored_local_key = StoredLocalKey { session_id, local_key: key };
				let serialized_data = serde_json::to_string(&stored_local_key)
					.map_err(|_| Error::new(ErrorKind::Other, "Serialization failed"))?;

				let encrypted_data = encrypt_data(serialized_data.into_bytes(), secret_key)
					.map_err(|e| Error::new(ErrorKind::Other, e))?;
				fs::write(path.clone(), &encrypted_data[..])?;

				debug!(target: "dkg_persistence", "Successfully stored local key for {:?}", &path);
				Ok(())
			} else {
				Err(Error::new(
					ErrorKind::Other,
					"Local key pair doesn't exist for sr25519 key".to_string(),
				))
			}
		} else {
			Err(Error::new(ErrorKind::Other, "Local keystore doesn't exist".to_string()))
		}
	} else {
		Err(Error::new(ErrorKind::Other, "Path not defined".to_string()))
	}
}

/// Loads a stored `StoredLocalKey` from the file system.
/// Expects the local keystore to exist in order to retrieve the file.
/// Expects there to be an sr25519 keypair with `KEY_TYPE` = `ACCOUNT`.
///
/// Uses the raw keypair as a seed for a secret key input to the XChaCha20Poly1305
/// encryption cipher.
#[allow(dead_code)]
pub(crate) fn load_stored_key(
	path: PathBuf,
	local_keystore: Option<&Arc<LocalKeystore>>,
	sr25519_public_key: sp_core::sr25519::Public,
) -> std::io::Result<StoredLocalKey> {
	if let Some(local_keystore) = local_keystore {
		let key_pair = local_keystore.as_ref().key_pair::<AppPair>(
			&Public::try_from(&sr25519_public_key.0[..])
				.unwrap_or_else(|_| panic!("Could not find keypair in local key store")),
		);

		if let Ok(Some(key_pair)) = key_pair {
			let secret_key = key_pair.to_raw_vec();

			let encrypted_data = fs::read(path)?;
			let decrypted_data = decrypt_data(encrypted_data, secret_key)
				.map_err(|e| Error::new(ErrorKind::Other, e))?;
			let stored_local_key: StoredLocalKey = serde_json::from_slice(&decrypted_data)
				.map_err(|_| Error::new(ErrorKind::Other, "Deserialization failed"))?;
			Ok(stored_local_key)
		} else {
			Err(Error::new(
				ErrorKind::Other,
				"Local key pair doesn't exist for sr25519 key".to_string(),
			))
		}
	} else {
		Err(Error::new(ErrorKind::Other, "Local keystore doesn't exist".to_string()))
	}
}
