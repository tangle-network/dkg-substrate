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
use crate::{
	worker::{DKGWorker, MAX_SUBMISSION_DELAY},
	Client,
};
use codec::Encode;
use dkg_primitives::types::{DKGError, RoundId};
use dkg_runtime_primitives::{
	crypto::AuthorityId,
	offchain::storage_keys::{
		AGGREGATED_PUBLIC_KEYS, AGGREGATED_PUBLIC_KEYS_AT_GENESIS, SUBMIT_GENESIS_KEYS_AT,
		SUBMIT_KEYS_AT,
	},
	AggregatedPublicKeys, DKGApi,
};
use sc_client_api::Backend;
use sp_api::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::traits::{Block, Header, NumberFor};

/// stores genesis or next aggregated public keys offchain
pub(crate) fn store_aggregated_public_keys<B, C, BE>(
	mut dkg_worker: &mut DKGWorker<B, C, BE>,
	is_genesis_round: bool,
	round_id: RoundId,
	keys: &AggregatedPublicKeys,
	current_block_number: NumberFor<B>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let maybe_offchain = dkg_worker.backend.offchain_storage();
	if maybe_offchain.is_none() {
		return Err(DKGError::GenericError { reason: "No offchain storage available".to_string() })
	}

	let offchain = maybe_offchain.unwrap();
	if is_genesis_round {
		dkg_worker.dkg_state.listening_for_active_pub_key = false;
		perform_storing_of_aggregated_public_keys(
			dkg_worker,
			offchain,
			keys,
			current_block_number,
			AGGREGATED_PUBLIC_KEYS_AT_GENESIS,
			SUBMIT_GENESIS_KEYS_AT,
		);
	} else {
		dkg_worker.dkg_state.listening_for_pub_key = false;
		perform_storing_of_aggregated_public_keys(
			dkg_worker,
			offchain,
			keys,
			current_block_number,
			AGGREGATED_PUBLIC_KEYS,
			SUBMIT_KEYS_AT,
		);
		let _ = dkg_worker.aggregated_public_keys.remove(&round_id);
	}

	Ok(())
}

/// stores the aggregated public keys
fn perform_storing_of_aggregated_public_keys<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	mut offchain: <BE as Backend<B>>::OffchainStorage,
	keys: &AggregatedPublicKeys,
	current_block_number: NumberFor<B>,
	aggregated_keys: &[u8],
	submit_keys: &[u8],
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	offchain.set(STORAGE_PREFIX, aggregated_keys, &keys.encode());
	let submit_at =
		dkg_worker.generate_delayed_submit_at(current_block_number.clone(), MAX_SUBMISSION_DELAY);
	if let Some(submit_at) = submit_at {
		offchain.set(STORAGE_PREFIX, submit_keys, &submit_at.encode());
	}
}
