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
use crate::{
	debug_logger::DebugLogger,
	utils::find_index,
	worker::{STORAGE_SET_RETRY_NUM},
	Client,
};
use codec::{Encode};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	offchain::storage_keys::OFFCHAIN_SIGNED_PROPOSALS,
	AuthoritySet, DKGApi, SignedProposalBatch,
};
use parking_lot::RwLock;
use rand::Rng;
use sc_client_api::Backend;
use sp_application_crypto::sp_core::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::traits::{Block, Get, Header, NumberFor};
use std::sync::Arc;


/// processes signed proposals and puts them in storage
pub(crate) fn save_signed_proposals_in_storage<
	B,
	C,
	BE,
	MaxProposalLength,
	MaxAuthorities,
	BatchId,
	MaxProposalsInBatch,
	MaxSignatureLength,
>(
	authority_public_key: &Public,
	current_validator_set: &Arc<RwLock<AuthoritySet<Public, MaxAuthorities>>>,
	latest_header: &Arc<RwLock<Option<B::Header>>>,
	backend: &Arc<BE>,
	signed_proposals: Vec<
		SignedProposalBatch<BatchId, MaxProposalLength, MaxProposalsInBatch, MaxSignatureLength>,
	>,
	logger: &DebugLogger,
) where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	MaxProposalLength:
		Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug + std::cmp::PartialEq,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + codec::Encode + codec::Decode,
	MaxProposalsInBatch:
		Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + codec::Encode + codec::Decode,
	MaxSignatureLength:
		Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + codec::Encode + codec::Decode,
	C::Api: DKGApi<
		B,
		AuthorityId,
		<<B as Block>::Header as Header>::Number,
		MaxProposalLength,
		MaxAuthorities,
	>,
{
	if signed_proposals.is_empty() {
		return
	}

	logger.debug("🕸️  saving signed proposal in offchain storage".to_string());

	if find_index::<AuthorityId>(
		&current_validator_set.read().authorities[..],
		authority_public_key,
	)
	.is_none()
	{
		return
	}

	let latest_header = latest_header.read().clone();

	// If the header is none, it means no block has been imported yet, so we can exit
	if latest_header.is_none() {
		return
	}

	let _current_block_number = {
		let header = latest_header.as_ref().expect("Should not happen, checked above");
		header.number()
	};

	if let Some(mut offchain) = backend.offchain_storage() {
		let old_val = offchain.get(STORAGE_PREFIX, OFFCHAIN_SIGNED_PROPOSALS);

		// TODO :: Cleanup

		// let mut prop_wrapper = match old_val.clone() {
		// 	Some(ser_props) => OffchainSignedProposalBatches::<
		// 		BatchId,
		// 		MaxProposalLength,
		// 		MaxProposalsInBatch,
		// 		MaxSignatureLength,
		// 	>::decode(&mut &ser_props[..])
		// 	.expect("Unable to decode offchain signed proposal!"),
		// 	None => Default::default(),
		// };

		// // The signed proposals are submitted in batches, since we want to try and limit
		// // duplicate submissions as much as we can, we add a random submission delay to each
		// // batch stored in offchain storage
		// let submit_at =
		// 	generate_delayed_submit_at::<B>(*current_block_number, MAX_SUBMISSION_DELAY);

		// // lets create a vector of the data of all the proposals currently in offchain storage
		// let current_list_of_saved_signed_proposals_data: Vec<Vec<u8>> = prop_wrapper
		// 	.clone()
		// 	.batches
		// 	.into_iter()
		// 	.map(|prop| prop.data().clone())
		// 	.collect::<Vec<_>>();

		// lets remove any duplicates
		// we need to compare the data to ensure that the proposal is a duplicate, otherwise the
		// signatures can be different for a same proposal
		// signed_proposals
		// 	.retain(|prop| !current_list_of_saved_signed_proposals_data.contains(&prop.data()));

		// prop_wrapper.batches.push(old_val)

		for _i in 1..STORAGE_SET_RETRY_NUM {
			if offchain.compare_and_set(
				STORAGE_PREFIX,
				OFFCHAIN_SIGNED_PROPOSALS,
				old_val.as_deref(),
				&signed_proposals.encode(),
			) {
				logger.debug(
					"🕸️  Successfully saved signed proposals in offchain storage".to_string(),
				);
				break
			}
		}
	}
}

/// Generate a random delay to wait before taking an action.
/// The delay is generated from a random number between 0 and `max_delay`.
pub fn generate_delayed_submit_at<B: Block>(
	start: NumberFor<B>,
	max_delay: u32,
) -> Option<<B::Header as Header>::Number> {
	let mut rng = rand::thread_rng();
	let submit_at = start + rng.gen_range(0u32..=max_delay).into();
	Some(submit_at)
}
