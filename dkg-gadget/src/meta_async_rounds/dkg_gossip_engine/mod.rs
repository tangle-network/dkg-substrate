use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{Stream, StreamExt};
use sc_network::PeerId;
use std::pin::Pin;

mod mock;
mod network;

pub use network::{GossipHandler, GossipHandlerController, NetworkGossipEngine};

/// A GossipEngine that can be used to send DKG messages.
///
/// in the core it is very simple, just two methods:
/// - `send` which will send a DKG message to a specific peer.
/// - `gossip` which will send a DKG message to all peers.
pub trait GossipEngineIface: Send + Sync {
	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError>;
	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError>;
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
