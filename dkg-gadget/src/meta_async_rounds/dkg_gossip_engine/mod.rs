use dkg_primitives::types::DKGError;
use sc_network::PeerId;

/// A GossipEngine that can be used to send DKG messages.
///
/// in the core it is very simple, just two methods:
/// - `send` which will send a DKG message to a specific peer.
/// - `gossip` which will send a DKG message to all peers.
pub trait GossipEngineIface: Send + Sync {
	fn send(&self, recipient: PeerId, message: &[u8]) -> Result<(), DKGError>;
	fn gossip(&self, message: &[u8]) -> Result<(), DKGError>;
}

/// A Stub implementation of the GossipEngineIface.
impl GossipEngineIface for () {
	fn send(&self, _recipient: PeerId, _message: &[u8]) -> Result<(), DKGError> {
		Ok(())
	}

	fn gossip(&self, _message: &[u8]) -> Result<(), DKGError> {
		Ok(())
	}
}
