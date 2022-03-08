// Copyright (C) 2020-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{collections::BTreeMap, time::Duration};

use sc_network::PeerId;
use sc_network_gossip::{ValidationResult, Validator, ValidatorContext};
use sp_runtime::traits::{Block, NumberFor};

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
	topic: B::Hash,
}

impl<B> GossipValidator<B>
where
	B: Block,
{
	pub fn new() -> GossipValidator<B> {
		GossipValidator { topic: dkg_topic::<B>() }
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
