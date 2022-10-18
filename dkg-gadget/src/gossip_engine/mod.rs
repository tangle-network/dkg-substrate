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

use std::pin::Pin;

use auto_impl::auto_impl;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{Stream, StreamExt};
use sc_network::PeerId;
use sp_arithmetic::traits::AtLeast32BitUnsigned;

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
	type Clock: AtLeast32BitUnsigned + Send + Sync;
	/// Send a DKG message to a specific peer.
	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError>;
	/// Send a DKG message to all peers.
	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError>;
	/// A stream that sends messages when they are ready to be polled from the message queue.
	fn message_available_notification(&self) -> Pin<Box<dyn Stream<Item = ()> + Send>>;
	/// Peek the front of the message queue.
	///
	/// Note that this will not remove the message from the queue, it will only return it. For
	/// removing the message from the queue, use `acknowledge_last_message`.
	///
	/// Returns `None` if there are no messages in the queue.
	fn peek_last_message(&self) -> Option<SignedDKGMessage<AuthorityId>>;
	/// Acknowledge the last message (the front of the queue) and mark it as processed, then removes
	/// it from the queue.
	fn acknowledge_last_message(&self);

	/// Clears the Message Queue.
	fn clear_queue(&self);
}

/// A Stub implementation of the GossipEngineIface.
impl GossipEngineIface for () {
	type Clock = u32;

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

	fn message_available_notification(&self) -> Pin<Box<dyn Stream<Item = ()> + Send>> {
		futures::stream::pending().boxed()
	}

	fn peek_last_message(&self) -> Option<SignedDKGMessage<AuthorityId>> {
		None
	}

	fn acknowledge_last_message(&self) {}

	fn clear_queue(&self) {}
}
