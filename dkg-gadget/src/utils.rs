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
use crate::{
	worker::{DKGWorker, ENGINE_ID},
	Client,
};
use dkg_primitives::{
	crypto::AuthorityId, rounds::MultiPartyECDSARounds, AuthoritySet, ConsensusLog, DKGApi, DKGThresholds,
};
use dkg_runtime_primitives::crypto::Public;
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_api::{BlockT as Block, HeaderT};
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_core::sr25519;
use sp_runtime::{generic::OpaqueDigestItemId, traits::Header};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub fn find_index<B: Eq>(queue: &[B], value: &B) -> Option<usize> {
	for (i, v) in queue.iter().enumerate() {
		if value == v {
			return Some(i)
		}
	}
	None
}

pub fn validate_threshold(n: u16, t: u16) -> u16 {
	let max_thresh = n - 1;
	if t >= 1 && t <= max_thresh {
		return t
	}

	return max_thresh
}

pub fn set_up_rounds<N: AtLeast32BitUnsigned + Copy>(
	authority_set: &AuthoritySet<AuthorityId>,
	public: &AuthorityId,
	local_key_path: Option<std::path::PathBuf>,
	reputations: &HashMap<AuthorityId, i64>,
	thresholds: DKGThresholds,
) -> MultiPartyECDSARounds<N> {
	let party_inx = find_index::<AuthorityId>(&authority_set.authorities[..], public).unwrap() + 1;
	// Compute the reputations of only the currently selected authorities for these rounds
	let mut authority_set_reputations = HashMap::new();
	authority_set.authorities.iter().for_each(|id| {
		authority_set_reputations.insert(id.clone(), *reputations.get(id).unwrap_or(&0i64));
	});
	// Generate the rounds object
	let rounds = MultiPartyECDSARounds::builder()
		.round_id(authority_set.id.clone())
		.party_index(u16::try_from(party_inx).unwrap())
		.threshold(thresholds.signature)
		.parties(u16::try_from(thresholds.keygen).unwrap())
		.local_key_path(local_key_path)
		.reputations(authority_set_reputations)
		.authorities(authority_set.authorities.clone())
		.build();

	rounds
}

/// Scan the `header` digest log for a DKG validator set change. Return either the new
/// validator set or `None` in case no validator set change has been signaled.
pub fn find_authorities_change<B>(
	header: &B::Header,
) -> Option<(AuthoritySet<AuthorityId>, AuthoritySet<AuthorityId>)>
where
	B: Block,
{
	let id = OpaqueDigestItemId::Consensus(&ENGINE_ID);

	header.digest().convert_first(|l| l.try_to(id).and_then(match_consensus_log))
}

fn match_consensus_log(
	log: ConsensusLog<AuthorityId>,
) -> Option<(AuthoritySet<AuthorityId>, AuthoritySet<AuthorityId>)> {
	match log {
		ConsensusLog::AuthoritiesChange {
			next_authorities: validator_set,
			next_queued_authorities,
		} => Some((validator_set, next_queued_authorities)),
		_ => None,
	}
}

pub(crate) fn is_next_authorities_or_rounds_empty<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	next_authorities: &AuthoritySet<Public>,
) -> bool
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if next_authorities.authorities.is_empty() {
		return true
	}

	if dkg_worker.dkg_state.curr_rounds.is_some() {
		if dkg_worker.dkg_state.curr_rounds.as_ref().unwrap().get_id() == next_authorities.id {
			return true
		}
	}

	false
}

pub(crate) fn is_queued_authorities_or_rounds_empty<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	queued_authorities: &AuthoritySet<Public>,
) -> bool
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	if queued_authorities.authorities.is_empty() {
		return true
	}

	if dkg_worker.dkg_state.next_rounds.is_some() {
		if dkg_worker.dkg_state.next_rounds.as_ref().unwrap().get_id() == queued_authorities.id {
			return true
		}
	}

	false
}

pub(crate) fn fetch_public_key<B, C, BE>(dkg_worker: &mut DKGWorker<B, C, BE>) -> Public
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	dkg_worker
		.key_store
		.authority_id(&dkg_worker.key_store.public_keys().unwrap())
		.unwrap_or_else(|| panic!("Halp"))
}

pub(crate) fn fetch_sr25519_public_key<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
) -> sp_core::sr25519::Public
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	dkg_worker
		.key_store
		.sr25519_authority_id(&dkg_worker.key_store.sr25519_public_keys().unwrap_or_default())
		.unwrap_or_else(|| panic!("Could not find sr25519 key in keystore"))
}

pub fn get_key_path(base_path: &Option<PathBuf>, path_str: &str) -> Option<PathBuf> {
	Some(base_path.as_ref().unwrap().join(path_str))
}
