use dkg_gadget::gossip_engine::GossipEngineIface;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::dummy_api::DummyApi;
use dkg_gadget::debug_logger::DebugLogger;
use dkg_runtime_primitives::crypto;

pub type PeerId = sc_network::PeerId;

#[derive(Clone)]
pub struct InMemoryGossipEngine {
	clients: Arc<Mutex<HashMap<PeerId, UnboundedSender<SignedDKGMessage<AuthorityId>>>>>,
	message_deliver_rx: Arc<Mutex<Option<UnboundedReceiver<SignedDKGMessage<AuthorityId>>>>>,
	this_peer: Option<PeerId>,
	this_peer_public_key: Option<AuthorityId>,
	// Maps Peer IDs to public keys
	mapping: Arc<Mutex<HashMap<PeerId, AuthorityId>>>,
	logger: Option<DebugLogger>,
}

impl Default for InMemoryGossipEngine {
	fn default() -> Self {
		Self::new()
	}
}

impl InMemoryGossipEngine {
	pub fn new() -> Self {
		Self {
			clients: Arc::new(Mutex::new(Default::default())),
			message_deliver_rx: Arc::new(Mutex::new(None)),
			this_peer: None,
			this_peer_public_key: None,
			mapping: Arc::new(Mutex::new(Default::default())),
			logger: None,
		}
	}

	/*
	fn public_to_peer_id(&self, public: AuthorityId) -> Option<PeerId> {
		let mapping = self.mapping.lock();
		for (peer_id, public_key) in mapping.iter() {
			if public_key == &public {
				return Some(*peer_id)
			}
		}

		None
	}*/

	// generates a new PeerId internally and adds to the hashmap
	pub fn clone_for_new_peer(
		&self,
		dummy_api: &DummyApi,
		n_blocks: u64,
		logger: &DebugLogger,
		this_peer: PeerId,
		public_key: crypto::Public,
	) -> Self {
		self.mapping.lock().insert(this_peer, public_key.clone());

		// by default, add this peer to the best authorities
		// TODO: make the configurable
		let mut lock = dummy_api.inner.write();
		// add +1 to allow calls for queued_authorities at block=n_blocks to not fail
		for x in 0..=(n_blocks + 1) {
			let entry = lock.authority_sets.entry(x).or_default();
			if !entry.contains(&public_key) {
				entry.force_push(public_key.clone());
			}
		}

		// give a new tx/rx handle to each new peer
		let (message_deliver_tx, message_deliver_rx) = tokio::sync::mpsc::unbounded_channel();
		self.clients.lock().insert(this_peer, message_deliver_tx);

		Self {
			clients: self.clients.clone(),
			message_deliver_rx: Arc::new(Mutex::new(Some(message_deliver_rx))),
			this_peer: Some(this_peer),
			this_peer_public_key: Some(public_key),
			mapping: self.mapping.clone(),
			logger: Some(logger.clone()),
		}
	}

	pub fn peer_id(&self) -> (&PeerId, &AuthorityId) {
		(self.this_peer.as_ref().unwrap(), self.this_peer_public_key.as_ref().unwrap())
	}
}

impl GossipEngineIface for InMemoryGossipEngine {
	type Clock = u128;

	/// Send a DKG message to a specific peer.
	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		let mut clients = self.clients.lock();
		let tx = clients
			.get_mut(&recipient)
			.ok_or_else(|| error(format!("Peer {recipient:?} does not exist")))?;
		tx.send(message).map_err(|_| error("Failed to send message"))?;
		Ok(())
	}

	/// Send a DKG message to all peers.
	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		let mut clients = self.clients.lock();
		let (this_peer, _) = self.peer_id();

		for (peer_id, tx) in clients.iter_mut() {
			if peer_id != this_peer {
				tx.send(message.clone())
					.map_err(|_| error("Failed to send broadcast message"))?;
			}
		}

		Ok(())
	}

	fn get_stream(&self) -> Option<UnboundedReceiver<SignedDKGMessage<AuthorityId>>> {
		self.message_deliver_rx.lock().take()
	}

	fn local_peer_id(&self) -> PeerId {
		*self.peer_id().0
	}

	fn logger(&self) -> &DebugLogger {
		self.logger.as_ref().unwrap()
	}
}

fn error<T: std::fmt::Debug>(err: T) -> DKGError {
	DKGError::GenericError { reason: format!("{err:?}") }
}
