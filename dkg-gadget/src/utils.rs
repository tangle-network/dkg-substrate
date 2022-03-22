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
	crypto::AuthorityId, rounds::MultiPartyECDSARounds, utils::get_best_authorities, AuthoritySet,
	ConsensusLog, DKGApi,
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
	signature_threshold: u16,
	keygen_threshold: u16,
	local_key_path: Option<std::path::PathBuf>,
) -> MultiPartyECDSARounds<N> {
	let best_authorities: Vec<AuthorityId> =
		get_best_authorities(keygen_threshold.into(), &authority_set.authorities, &reputations)
			.iter()
			.map(|(_, key)| key.clone())
			.collect();
	let party_inx = find_index::<AuthorityId>(&best_authorities[..], public).unwrap() + 1;
	// Generate the rounds object
	let rounds = MultiPartyECDSARounds::builder()
		.round_id(authority_set.id.clone())
		.party_index(u16::try_from(party_inx).unwrap())
		.threshold(signature_threshold)
		.parties(keygen_threshold)
		.local_key_path(local_key_path)
		.authorities(best_authorities)
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

	if dkg_worker.rounds.is_some() {
		if dkg_worker.rounds.as_ref().unwrap().get_id() == next_authorities.id {
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

	if dkg_worker.next_rounds.is_some() {
		if dkg_worker.next_rounds.as_ref().unwrap().get_id() == queued_authorities.id {
			return true
		}
	}

	false
}

pub fn get_key_path(base_path: &Option<PathBuf>, path_str: &str) -> Option<PathBuf> {
	Some(base_path.as_ref().unwrap().join(path_str))
}
