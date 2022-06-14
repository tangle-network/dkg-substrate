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
	gossip_engine::GossipEngineIface,
	utils::find_index,
	worker::{DKGWorker, MAX_SUBMISSION_DELAY, STORAGE_SET_RETRY_NUM},
	Client,
};
use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::OFFCHAIN_SIGNED_PROPOSALS, DKGApi,
	OffchainSignedProposals, Proposal,
};
use log::debug;
use sc_client_api::Backend;
use sp_application_crypto::sp_core::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::traits::{Block, Header, NumberFor};

/// processes signed proposals and puts them in storage
pub(crate) fn save_signed_proposals_in_storage<B, BE, C, GE>(
	dkg_worker: &mut DKGWorker<B, BE, C, GE>,
	signed_proposals: Vec<Proposal>,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if signed_proposals.is_empty() {
		return
	}

	debug!(target: "dkg", "🕸️  saving signed proposal in offchain storage");

	let public = dkg_worker.get_authority_public_key();

	if find_index::<AuthorityId>(&dkg_worker.current_validator_set.authorities[..], &public)
		.is_none()
	{
		return
	}

	// If the header is none, it means no block has been imported yet, so we can exit
	if dkg_worker.latest_header.is_none() {
		return
	}

	let current_block_number = {
		let header = dkg_worker.latest_header.as_ref().unwrap();
		header.number()
	};

	if let Some(mut offchain) = dkg_worker.backend.offchain_storage() {
		let old_val = offchain.get(STORAGE_PREFIX, OFFCHAIN_SIGNED_PROPOSALS);

		let mut prop_wrapper = match old_val.clone() {
			Some(ser_props) =>
				OffchainSignedProposals::<NumberFor<B>>::decode(&mut &ser_props[..]).unwrap(),
			None => Default::default(),
		};

		// The signed proposals are submitted in batches, since we want to try and limit
		// duplicate submissions as much as we can, we add a random submission delay to each
		// batch stored in offchain storage
		let submit_at =
			dkg_worker.generate_delayed_submit_at(*current_block_number, MAX_SUBMISSION_DELAY);

		if let Some(submit_at) = submit_at {
			prop_wrapper.proposals.push((signed_proposals, submit_at))
		};

		for _i in 1..STORAGE_SET_RETRY_NUM {
			if offchain.compare_and_set(
				STORAGE_PREFIX,
				OFFCHAIN_SIGNED_PROPOSALS,
				old_val.as_deref(),
				&prop_wrapper.encode(),
			) {
				debug!(target: "dkg", "🕸️  Successfully saved signed proposals in offchain storage");
				break
			}
		}
	}
}
