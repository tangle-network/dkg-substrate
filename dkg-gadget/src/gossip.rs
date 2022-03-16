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

use sc_network::PeerId;
use sc_network_gossip::{ValidationResult, Validator, ValidatorContext};
use sp_runtime::traits::Block;

use codec::Decode;
use log::{debug, error, trace};

use crate::types::dkg_topic;
use dkg_primitives::types::SignedDKGMessage;
use dkg_runtime_primitives::crypto::Public;

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
}

impl<B> GossipValidator<B>
where
	B: Block,
{
	pub fn new() -> GossipValidator<B> {
		GossipValidator { _phantom: Default::default() }
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
		debug!(target: "dkg", "🕸️  Got a signed message ({:?} bytes) from: {:?}", data_copy.len(), sender);
		match SignedDKGMessage::<Public>::decode(&mut data_copy) {
			Ok(msg) => {
				trace!(target: "dkg", "🕸️  Got a signed dkg message: {:?}, from: {:?}", msg, sender);
				return ValidationResult::ProcessAndKeep(dkg_topic::<B>())
			},
			Err(e) => {
				error!(target: "dkg", "🕸️  Got invalid signed dkg message: {:?}, from: {:?}", e, sender);
			},
		}

		ValidationResult::Discard
	}
}

