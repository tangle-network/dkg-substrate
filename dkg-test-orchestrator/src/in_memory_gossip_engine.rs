use dkg_gadget::gossip_engine::GossipEngineIface;
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::Stream;
use parking_lot::Mutex;
use std::{
	collections::{HashMap, VecDeque},
	pin::Pin,
	sync::Arc,
};

use sp_keystore::SyncCryptoStore;

use crate::{client::MultiSubscribableStream, dummy_api::DummyApi};
use dkg_gadget::debug_logger::DebugLogger;
use dkg_runtime_primitives::{crypto, KEY_TYPE};

pub type PeerId = sc_network::PeerId;

#[derive(Clone)]
pub struct InMemoryGossipEngine {
	clients: Arc<Mutex<HashMap<PeerId, VecDeque<SignedDKGMessage<AuthorityId>>>>>,
	notifier: Arc<Mutex<HashMap<PeerId, MultiSubscribableStream<()>>>>,
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
			notifier: Arc::new(Mutex::new(Default::default())),
			this_peer: None,
			this_peer_public_key: None,
			mapping: Arc::new(Mutex::new(Default::default())),
			logger: None,
		}
	}

	#[allow(dead_code)]
	fn public_to_peer_id(&self, public: AuthorityId) -> Option<PeerId> {
		let mapping = self.mapping.lock();
		for (peer_id, public_key) in mapping.iter() {
			if public_key == &public {
				return Some(*peer_id)
			}
		}

		None
	}

	// generates a new PeerId internally and adds to the hashmap
	pub fn clone_for_new_peer(
		&self,
		dummy_api: &DummyApi,
		n_blocks: u64,
		keyring: dkg_gadget::keyring::Keyring,
		key_store: &dyn SyncCryptoStore,
		logger: &DebugLogger,
	) -> Self {
		let public_key: crypto::Public =
			SyncCryptoStore::ecdsa_generate_new(key_store, KEY_TYPE, Some(&keyring.to_seed()))
				.ok()
				.unwrap()
				.into();

		let this_peer = PeerId::random();
		let stream = MultiSubscribableStream::new("stream notifier", logger.clone());
		self.mapping.lock().insert(this_peer, public_key.clone());
		assert!(self.clients.lock().insert(this_peer, Default::default()).is_none());
		self.notifier.lock().insert(this_peer, stream);

		// by default, add this peer to the best authorities
		// TODO: make the configurable
		let mut lock = dummy_api.inner.write();
		// add +1 to allow calls for queued_authorities at block=n_blocks to not fail
		for x in 0..=(n_blocks + 1) {
			lock.authority_sets.entry(x).or_default().force_push(public_key.clone());
		}

		Self {
			clients: self.clients.clone(),
			notifier: self.notifier.clone(),
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

	fn logger(&self) -> &DebugLogger {
		self.logger.as_ref().unwrap()
	}

	fn local_peer_id(&self) -> PeerId {
		*self.peer_id().0
	}

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
		tx.push_back(message);

		// notify the receiver
		self.notifier.lock().get(&recipient).unwrap().send(());
		Ok(())
	}

	/// Send a DKG message to all peers.
	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		let mut clients = self.clients.lock();
		let notifiers = self.notifier.lock();
		let (this_peer, _) = self.peer_id();

		for (peer_id, tx) in clients.iter_mut() {
			if peer_id != this_peer {
				tx.push_back(message.clone());
			}
		}

		for (peer_id, notifier) in notifiers.iter() {
			if peer_id != this_peer {
				notifier.send(());
			}
		}

		Ok(())
	}
	/// A stream that sends messages when they are ready to be polled from the message queue.
	fn message_available_notification(&self) -> Pin<Box<dyn Stream<Item = ()> + Send>> {
		let (this_peer, _) = self.peer_id();
		let rx = self.notifier.lock().get(this_peer).unwrap().subscribe();
		Box::pin(rx) as _
	}
	/// Peek the front of the message queue.
	///
	/// Note that this will not remove the message from the queue, it will only return it. For
	/// removing the message from the queue, use `acknowledge_last_message`.
	///
	/// Returns `None` if there are no messages in the queue.
	fn peek_last_message(&self) -> Option<SignedDKGMessage<AuthorityId>> {
		let (this_peer, _) = self.peer_id();
		let clients = self.clients.lock();
		clients.get(this_peer).unwrap().front().cloned()
	}
	/// Acknowledge the last message (the front of the queue) and mark it as processed, then
	/// removes it from the queue.
	fn acknowledge_last_message(&self) {
		let (this_peer, _) = self.peer_id();
		let mut clients = self.clients.lock();
		clients.get_mut(this_peer).unwrap().pop_front();
	}

	/// Clears the Message Queue.
	fn clear_queue(&self) {
		let (this_peer, _) = self.peer_id();
		let mut clients = self.clients.lock();
		clients.get_mut(this_peer).unwrap().clear();
	}
}

fn error<T: std::fmt::Debug>(err: T) -> DKGError {
	DKGError::GenericError { reason: format!("{err:?}") }
}
