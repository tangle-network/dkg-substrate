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
use dkg_primitives::{crypto::AuthorityId, types::DKGError, AuthoritySet, ConsensusLog};
use sp_api::{BlockT as Block, HeaderT};
use sp_runtime::generic::OpaqueDigestItemId;
use std::{fmt::Debug, future::Future, path::PathBuf};

pub trait SendFuture<'a, Out: 'a>: Future<Output = Result<Out, DKGError>> + Send + 'a {}
impl<'a, T, Out: Debug + Send + 'a> SendFuture<'a, Out> for T where
	T: Future<Output = Result<Out, DKGError>> + Send + 'a
{
}

/// Finds the index of a value in a vector. Returns None if the value is not found.
pub fn find_index<B: Eq>(queue: &[B], value: &B) -> Option<usize> {
	queue.iter().position(|v| value == v)
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

#[cfg(feature = "outbound-inspection")]
pub(crate) fn inspect_outbound(ty: &'static str, serialized_len: usize) {
	use parking_lot::Mutex;
	use std::collections::HashMap;

	static MAP: Mutex<Option<HashMap<&'static str, Vec<u32>>>> = parking_lot::const_mutex(None);
	let mut lock = MAP.lock();

	if lock.is_none() {
		*lock = Some(HashMap::new())
	}

	let map = lock.as_mut().unwrap();

	map.entry(ty).or_default().push(serialized_len as u32);

	for (ty, history) in map.iter() {
		log::debug!(target: "dkg", "History for {}: \
			total count={}, \
			first={:?}, \
			latest={:?}, \
			lifetime_delta={:?}, \
			max={:?}",
		ty,
		history.len(),
		history.first(),
		history.last(),
		history.last().and_then(|latest| history.first().map(|first| *latest as i64 - *first as i64)),
		history.iter().max());
	}
}

#[cfg(not(feature = "outbound-inspection"))]
pub(crate) fn inspect_outbound(_ty: &str, _serialized_len: usize) {}
