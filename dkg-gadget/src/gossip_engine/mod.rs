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
use dkg_primitives::{
	crypto::AuthoritySignature,
	types::{DKGError, SignedDKGMessage},
};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{Stream, StreamExt};
use sc_network::PeerId;
use sp_application_crypto::RuntimeAppPublic;
use sp_arithmetic::traits::AtLeast32BitUnsigned;

/// A Gossip Engine for DKG, that uses [`sc_network::NetworkService`] as a backend.
mod network;

pub use network::{GossipHandler, GossipHandlerController, NetworkGossipEngineBuilder};

use crate::{debug_logger::DebugLogger, worker::KeystoreExt, DKGKeystore};

/// A GossipEngine that can be used to send DKG messages.
///
/// in the core it is very simple, just two methods:
/// - `send` which will send a DKG message to a specific peer.
/// - `gossip` which will send a DKG message to all peers.
/// - `stream` which will return a stream of DKG messages.
#[auto_impl(Arc,Box,&)]
pub trait GossipEngineIface: Send + Sync + 'static {
	type Clock: AtLeast32BitUnsigned + Send + Sync + 'static;
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

	fn local_peer_id(&self) -> PeerId;
	fn logger(&self) -> &DebugLogger;
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

	fn local_peer_id(&self) -> PeerId {
		PeerId::random()
	}

	fn logger(&self) -> &DebugLogger {
		panic!()
	}
}

/// A Handshake message that is sent when a peer connects to us, to verify that the peer Id (which
/// is the sender) is the owner of the authority id.
/// This is used to prevent a malicious peer from impersonating another authority.
/// **Notes:**
/// - The peer id is the id of the peer that sent the handshake message.
/// - The authority id is the id of the authority that the peer claims to be.
/// - The signature is the signature of (peer id, authority id) by the authority id.
/// - The signature is used to verify that the authority id is the owner of the peer id.
#[derive(Debug, Clone, PartialEq, Eq, codec::Encode, codec::Decode)]
pub struct HandshakeMessage {
	pub authority_id: AuthorityId,
	pub peer_id: Vec<u8>,
	pub signature: AuthoritySignature,
}

impl HandshakeMessage {
	/// Create a new handshake message.
	///
	/// This will sign the peer id and authority id with the authority id.
	/// Returns an error if the authority id does not have a corresponding private key in the
	/// keystore.
	pub fn try_new(keystore: &DKGKeystore, peer_id: PeerId) -> Result<Self, DKGError> {
		let peer_id = peer_id.to_bytes();
		let authority_id = keystore.get_authority_public_key();
		let msg = peer_id
			.clone()
			.into_iter()
			.chain(authority_id.to_raw_vec().into_iter())
			.collect::<Vec<_>>();
		let signature = keystore.sign(&authority_id, &msg).map_err(|e| {
			DKGError::CriticalError { reason: format!("Failed to sign handshake message: {e}") }
		})?;
		Ok(Self { authority_id, peer_id, signature })
	}

	/// Verify that the handshake message is valid.
	///
	/// This will check that:
	/// 1. The peer id is the same as the sender peer id.
	/// 2. The signature is valid.
	/// 3. The authority id is the owner of the peer id.
	pub fn is_valid(&self, sender_peer_id: PeerId) -> Result<bool, DKGError> {
		// Check that the peer id is the same as the sender peer id.
		let msg_peer_id = match PeerId::try_from(self.peer_id.clone()) {
			Ok(peer_id) => peer_id,
			Err(_) => return Err(DKGError::InvalidPeerId),
		};
		if msg_peer_id != sender_peer_id {
			return Ok(false)
		}
		let msg = self
			.peer_id
			.clone()
			.into_iter()
			.chain(self.authority_id.to_raw_vec().into_iter())
			.collect::<Vec<_>>();

		// Verify the signature.
		let msg = dkg_primitives::keccak_256(&msg);
		let valid = sp_core::ecdsa::Pair::verify_prehashed(
			&self.signature.clone().into(),
			&msg,
			&self.authority_id.clone().into(),
		);
		Ok(valid)
	}
}

#[derive(Debug, Clone, codec::Decode, codec::Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum DKGNetworkMessage {
	Handshake(HandshakeMessage),
	DKGMessage(SignedDKGMessage<AuthorityId>),
}
