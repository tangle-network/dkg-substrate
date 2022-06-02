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

//! Webb Custom DKG Gossip Engine.

use std::collections::HashMap;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{Stream, StreamExt};
use sc_network::PeerId;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use parking_lot::RwLock;
use auto_impl::auto_impl;

/// A Mock Gossip Engine for DKG.
mod mock;
/// A Gossip Engine for DKG, that uses [`sc_network::NetworkService`] as a backend.
mod network;

pub use network::{GossipHandler, GossipHandlerController, NetworkGossipEngineBuilder};

/// A GossipEngine that can be used to send DKG messages.
///
/// in the core it is very simple, just two methods:
/// - `send` which will send a DKG message to a specific peer.
/// - `gossip` which will send a DKG message to all peers.
/// - `stream` which will return a stream of DKG messages.
#[auto_impl(Arc,Box,&)]
pub trait GossipEngineIface: Send + Sync {
	/// Send a DKG message to a specific peer.
	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError>;
	/// Send a DKG message to all peers.
	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError>;
	/// Returns a stream of DKG messages, that are received from the network.
	fn stream(&self) -> Pin<Box<dyn Stream<Item = SignedDKGMessage<AuthorityId>> + Send>>;
	/// A list of timestamps of the last received message for each peer
	fn receive_timestamps(&self) -> Option<&ReceiveTimestamp>;
}

pub type ReceiveTimestamp = Arc<RwLock<HashMap<PeerId, Instant>>>;

/// A Stub implementation of the GossipEngineIface.
impl GossipEngineIface for () {
	fn send(
		&self,
		_recipient: PeerId,
		_message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		Ok(())
	}

	fn gossip(&self, _message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		Ok(())
	}

	fn stream(&self) -> Pin<Box<dyn Stream<Item = SignedDKGMessage<AuthorityId>> + Send>> {
		futures::stream::pending().boxed()
	}

	fn receive_timestamps(&self) -> Option<&ReceiveTimestamp> {
		None
	}
}
