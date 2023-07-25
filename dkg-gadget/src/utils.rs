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
use crate::worker::{ProtoStageType, ENGINE_ID};
use dkg_primitives::{
	crypto::AuthorityId, types::DKGError, AuthoritySet, ConsensusLog, MaxAuthorities,
};
use sp_api::{BlockT as Block, HeaderT};
use sp_runtime::generic::OpaqueDigestItemId;
use std::{collections::HashMap, fmt::Debug, future::Future, sync::Arc};

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
) -> Option<(AuthoritySet<AuthorityId, MaxAuthorities>, AuthoritySet<AuthorityId, MaxAuthorities>)>
where
	B: Block,
{
	let id = OpaqueDigestItemId::Consensus(&ENGINE_ID);

	header.digest().convert_first(|l| l.try_to(id).and_then(match_consensus_log))
}

/// Matches a `ConsensusLog` for a DKG validator set change.
fn match_consensus_log(
	log: ConsensusLog<AuthorityId, MaxAuthorities>,
) -> Option<(AuthoritySet<AuthorityId, MaxAuthorities>, AuthoritySet<AuthorityId, MaxAuthorities>)>
{
	match log {
		ConsensusLog::AuthoritiesChange { active: authority_set, queued: queued_authority_set } =>
			Some((authority_set, queued_authority_set)),
		_ => None,
	}
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
		dkg_logging::debug!(target: "dkg_gadget", "History for {}: \
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

use crate::async_protocols::KeygenPartyId;
use futures::task::Context;
use tokio::{
	macros::support::{Pin, Poll},
	task::{JoinError, JoinHandle},
};

/// Ensures that if a panic occurs in a task, the panic backtrace prints
pub struct ExplicitPanicFuture<F> {
	future: JoinHandle<F>,
}

impl<F> ExplicitPanicFuture<F> {
	pub fn new(future: JoinHandle<F>) -> Self {
		Self { future }
	}

	pub fn abort(&self) {
		self.future.abort();
	}
}

impl<F> Future for ExplicitPanicFuture<F> {
	type Output = Result<F, JoinError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match futures::ready!(Pin::new(&mut self.future).poll(cx)) {
			Err(err) =>
				if err.is_panic() {
					std::panic::panic_any(err.into_panic())
				} else {
					Poll::Ready(Err(err))
				},

			res => Poll::Ready(res),
		}
	}
}

pub fn bad_actors_to_authorities<T: Into<usize>>(
	bad_actors: Vec<T>,
	mapping: &Arc<HashMap<usize, AuthorityId>>,
) -> Vec<AuthorityId> {
	let mut ret = Vec::new();
	for bad_actor in bad_actors {
		if let Some(authority) = mapping.get(&bad_actor.into()) {
			ret.push(authority.clone());
		}
	}

	ret
}

pub fn generate_authority_mapping(
	authorities: &Vec<(KeygenPartyId, AuthorityId)>,
	proto_ty: ProtoStageType,
) -> Arc<HashMap<usize, AuthorityId>> {
	match proto_ty {
		ProtoStageType::KeygenGenesis | ProtoStageType::KeygenStandard => {
			// Simple mapping. Map the index via as_ref to get the u16
			Arc::new(
				authorities
					.clone()
					.into_iter()
					.map(|(idx, auth)| ((*idx.as_ref()) as usize, auth))
					.collect(),
			)
		},

		ProtoStageType::Signing { .. } => {
			// More complex. We take each party_i = keygen_i from above, then, convert to an offline
			// stage index via party_i.try_to_offline_party_id
			let mut ret = HashMap::new();
			let authorities_only: Vec<KeygenPartyId> = authorities.iter().map(|r| r.0).collect();
			for (idx, auth) in authorities {
				if let Ok(offline_index) = idx.try_to_offline_party_id(&authorities_only) {
					ret.insert(*offline_index.as_ref() as usize, auth.clone());
				}
			}

			Arc::new(ret)
		},
	}
}
