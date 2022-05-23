//! Webb Custom DKG Gossip Engine.

use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{Stream, StreamExt};
use sc_network::PeerId;
use std::pin::Pin;
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
}

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
}
