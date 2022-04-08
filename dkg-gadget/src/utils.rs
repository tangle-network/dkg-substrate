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
use crate::worker::ENGINE_ID;
use dkg_primitives::{
	crypto::AuthorityId, rounds::MultiPartyECDSARounds, types::RoundId, AuthoritySet, ConsensusLog,
};
use sp_api::{BlockT as Block, HeaderT};
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_runtime::generic::OpaqueDigestItemId;
use std::path::PathBuf;

/// Finds the index of a value in a vector. Returns None if the value is not found.
pub fn find_index<B: Eq>(queue: &[B], value: &B) -> Option<usize> {
	for (i, v) in queue.iter().enumerate() {
		if value == v {
			return Some(i)
		}
	}
	None
}

/// Sets up the Multi-party ECDSA rounds struct used to receive, process, and handle
/// incoming and outgoing DKG related messages for key generation, offline stage creation,
/// and signing. The rounds struct is used to handle the execution of a single round of the DKG
/// and should be created for each session.
///
/// The rounds are intended to be run only by `best_authorities` that are selected from
/// an externally provided set of reputations. Rounds are parameterized for a `t-of-n` threshold
/// - `signature_threshold` represents `t`
/// - `keygen_threshold` represents `n`
///
/// We provide an optional `local_key_path` to this struct so that it may save the generated
/// DKG public / local key to disk. Caching of this key is critical to persistent storage and
/// resuming the worker from a machine failure.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn set_up_rounds<N: AtLeast32BitUnsigned + Copy>(
	best_authorities: &[AuthorityId],
	authority_set_id: RoundId,
	public: &AuthorityId,
	signature_threshold: u16,
	keygen_threshold: u16,
	local_key_path: Option<std::path::PathBuf>,
	jailed_signers: &[AuthorityId],
) -> MultiPartyECDSARounds<N> {
	let party_inx = find_index::<AuthorityId>(best_authorities, public).unwrap() + 1;
	// Generate the rounds object
	MultiPartyECDSARounds::builder()
		.round_id(authority_set_id)
		.party_index(u16::try_from(party_inx).unwrap())
		.threshold(signature_threshold)
		.parties(keygen_threshold)
		.local_key_path(local_key_path)
		.authorities(best_authorities.to_vec())
		.jailed_signers(jailed_signers.to_vec())
		.build()
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

/// Matches a `ConsensusLog` for a DKG validator set change.
fn match_consensus_log(
	log: ConsensusLog<AuthorityId>,
) -> Option<(AuthoritySet<AuthorityId>, AuthoritySet<AuthorityId>)> {
	match log {
		ConsensusLog::AuthoritiesChange { active: authority_set, queued: queued_authority_set } =>
			Some((authority_set, queued_authority_set)),
		_ => None,
	}
}

/// Returns an optional key path if a base path is provided.
///
/// This path is used to store the DKG public key / local key
/// generated through the multi-party threshold ECDSA key generation.
pub fn get_key_path(base_path: &Option<PathBuf>, path_str: &str) -> Option<PathBuf> {
	base_path.as_ref().map(|path| path.join(path_str))
}
