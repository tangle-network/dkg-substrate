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

use codec::Encode;
use parking_lot::RwLock;
use sc_network::PeerId;
use sc_network_gossip::{ValidationResult, Validator, ValidatorContext};
use sp_core::keccak_256;
use sp_runtime::traits::Block;
use std::collections::BTreeMap;

use codec::Decode;
use log::{error, trace};

use crate::types::dkg_topic;
use dkg_primitives::types::SignedDKGMessage;
use dkg_runtime_primitives::crypto::Public;

pub const MAX_LIVE_GOSSIP_ROUNDS: u8 = 3;

struct Seen {
	seen: BTreeMap<[u8; 32], u8>,
}

impl Seen {
	pub fn new() -> Self {
		Self { seen: BTreeMap::new() }
	}

	/// Create new seen entry.
	fn insert(&mut self, hash: [u8; 32]) {
		self.seen.entry(hash).or_insert(1);
	}

	/// Increment new seen count
	fn increment(&mut self, hash: [u8; 32]) {
		#[allow(clippy::map_entry)]
		if self.seen.contains_key(&hash) {
			// Mutate if exists
			let count = self.get_seen_count(&hash);
			self.seen.insert(hash, count + 1);
		} else {
			// Insert if doesn't exist
			self.insert(hash);
		}
	}

	/// Return true if `hash` has been seen `MAX_LIVE_GOSSIP_ROUNDS` times.
	fn is_valid(&self, hash: &[u8]) -> bool {
		self.get_seen_count(hash) <= MAX_LIVE_GOSSIP_ROUNDS
	}

	/// Check if `hash` is already part of seen messages
	fn get_seen_count(&self, hash: &[u8]) -> u8 {
		self.seen.get(hash).copied().unwrap_or(0)
	}
}

/// DKG gossip validator
///
/// Validate DKG gossip messages and limit the number of live DKG voting rounds.
///
/// Allows messages from last [`MAX_LIVE_GOSSIP_ROUNDS`] to flow, everything else gets
/// rejected/expired.
///
/// All messaging is handled in a single DKG global topic.
pub(crate) struct GossipValidator<B>
where
	B: Block,
{
	_phantom: std::marker::PhantomData<B>,
	seen: RwLock<Seen>,
}

impl<B> GossipValidator<B>
where
	B: Block,
{
	pub fn new() -> GossipValidator<B> {
		GossipValidator { seen: RwLock::new(Seen::new()), _phantom: Default::default() }
	}
}

impl<B> Validator<B> for GossipValidator<B>
where
	B: Block,
{
	fn validate(
		&self,
		_context: &mut dyn ValidatorContext<B>,
		sender: &PeerId,
		data: &[u8],
	) -> ValidationResult<B::Hash> {
		let mut data_copy = data;
		match SignedDKGMessage::<Public>::decode(&mut data_copy) {
			Ok(msg) => {
				trace!(target: "dkg", "🕸️  Got a signed dkg message: {:?}, from: {:?}", msg, sender);
				let hash = keccak_256(&msg.encode());
				if self.seen.read().is_valid(&hash) {
					log::debug!(target: "dkg", "🕸️  Seen message: ({:?} times)", self.seen.read().get_seen_count(&hash));
					self.seen.write().increment(hash);
					return ValidationResult::ProcessAndKeep(dkg_topic::<B>())
				} else {
					return ValidationResult::ProcessAndDiscard(dkg_topic::<B>())
				}
			},
			Err(e) => {
				error!(target: "dkg", "🕸️  Got invalid signed dkg message: {:?}, from: {:?}", e, sender);
			},
		}

		ValidationResult::Discard
	}
}
