// This file is part of Webb.

// Copyright (C) 2021 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{gossip_engine::GossipEngineIface, worker::DKGWorker, Client};
use dkg_runtime_primitives::{
	crypto::AuthorityId,
	offchain::storage_keys::{
		AGGREGATED_PUBLIC_KEYS, AGGREGATED_PUBLIC_KEYS_AT_GENESIS, OFFCHAIN_PUBLIC_KEY_SIG,
		SUBMIT_GENESIS_KEYS_AT, SUBMIT_KEYS_AT,
	},
	DKGApi,
};
use log::debug;
use sc_client_api::Backend;
use sp_application_crypto::sp_core::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header, NumberFor},
};

/// cleans offchain storage at interval
pub(crate) fn listen_and_clear_offchain_storage<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	header: &B::Header,
) where
	B: Block,
	GE: GossipEngineIface + 'static,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
{
	let at: BlockId<B> = BlockId::hash(header.hash());
	let next_dkg_public_key = dkg_worker.client.runtime_api().next_dkg_pub_key(&at);
	let dkg_public_key = dkg_worker.client.runtime_api().dkg_pub_key(&at).ok();
	let public_key_sig =
		dkg_worker.client.runtime_api().next_pub_key_sig(&at).ok().unwrap_or_default();

	let offchain = dkg_worker.backend.offchain_storage();

	if let Some(mut offchain) = offchain {
		if let Ok(Some(_key)) = next_dkg_public_key {
			if offchain.get(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS).is_some() {
				debug!(target: "dkg", "cleaned offchain storage, next_public_key: {:?}", _key);
				offchain.remove(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS);

				offchain.remove(STORAGE_PREFIX, SUBMIT_KEYS_AT);
			}
		}

		if let Some(_key) = dkg_public_key {
			if offchain.get(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS_AT_GENESIS).is_some() &&
				!_key.1.is_empty()
			{
				debug!(target: "dkg", "cleaned offchain storage, genesis_pub_key: {:?}", _key);
				offchain.remove(STORAGE_PREFIX, AGGREGATED_PUBLIC_KEYS_AT_GENESIS);

				offchain.remove(STORAGE_PREFIX, SUBMIT_GENESIS_KEYS_AT);
			}
		}

		if let Some(_sig) = public_key_sig {
			if offchain.get(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG).is_some() {
				debug!(target: "dkg", "cleaned offchain storage, next_pub_key_sig: {:?}", _sig);
				offchain.remove(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG);
			}
		}
	}
}

/// cleans offchain storage at interval
pub(crate) fn is_offchain_storage_empty<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	storage_key: &[u8],
) -> bool
where
	B: Block,
	GE: GossipEngineIface + 'static,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
{
	let offchain = dkg_worker.backend.offchain_storage();
	if offchain.is_some() {
		return offchain.unwrap().get(STORAGE_PREFIX, storage_key).is_some()
	}

	return true
}
