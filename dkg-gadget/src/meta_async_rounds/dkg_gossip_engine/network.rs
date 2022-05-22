use crate::metrics::Metrics;
use codec::{Decode, Encode};
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{
	channel::mpsc, stream::StreamExt as _, FutureExt, SinkExt, Stream, StreamExt, TryFutureExt,
	TryStreamExt,
};
use linked_hash_set::LinkedHashSet;
use log::{debug, trace, warn};
use sc_network::{config, error, multiaddr, Event, ExHashT, NetworkService, PeerId};
use sp_runtime::traits::Block;
use std::{
	borrow::Cow,
	collections::{hash_map::Entry, HashMap},
	hash::Hash,
	iter,
	num::NonZeroUsize,
	pin::Pin,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
};
use tokio::sync::broadcast;

pub struct NetworkGossipEngine;

impl NetworkGossipEngine {
	/// Create a new network gossip engine.
	pub fn new() -> Self {
		Self
	}

	/// Returns the configuration of the set to put in the network configuration.
	pub fn set_config() -> config::NonDefaultSetConfig {
		config::NonDefaultSetConfig {
			notifications_protocol: crate::DKG_PROTOCOL_NAME.into(),
			fallback_names: Vec::new(),
			max_notification_size: MAX_MESSAGE_SIZE,
			set_config: config::SetConfig {
				in_peers: 0,
				out_peers: 0,
				reserved_nodes: Vec::new(),
				non_reserved_mode: config::NonReservedPeerMode::Deny,
			},
		}
	}

	/// Turns the engine into the actual handler. Returns a controller that allows controlling
	/// the behaviour of the handler while it's running.
	///
	/// Important: the gossip handler is initially disabled and doesn't gossip messages.
	/// You must call [`GossipHandlerController::set_gossip_enabled`] to enable it.
	pub(crate) fn build<B: Block>(
		self,
		service: Arc<NetworkService<B, B::Hash>>,
		metrics: Option<Metrics>,
	) -> error::Result<(GossipHandler<B>, GossipHandlerController)> {
		let event_stream = service.event_stream("dkg-handler").boxed();
		let (to_handler, from_controller) = broadcast::channel(1000);
		let (to_controller, _) = broadcast::channel(1000);

		let gossip_enabled = Arc::new(AtomicBool::new(false));

		let handler = GossipHandler {
			protocol_name: crate::DKG_PROTOCOL_NAME.into(),
			to_controller: to_controller.clone(),
			to_handler: to_handler.clone(),
			pending_messages_peers: HashMap::new(),
			gossip_enabled: gossip_enabled.clone(),
			service,
			event_stream,
			peers: HashMap::new(),
			from_controller,
			metrics,
		};

		let controller = GossipHandlerController { to_handler, to_controller, gossip_enabled };

		Ok((handler, controller))
	}
}

/// Maximum number of known messages hashes to keep for a peer.
const MAX_KNOWN_MESSAGES: usize = 10240; // ~300kb per peer + overhead.

/// Maximum allowed size for a DKG Signed Message notification.
const MAX_MESSAGE_SIZE: u64 = 16 * 1024 * 1024;

/// Maximum number of messages request we keep at any moment.
const MAX_PENDING_MESSAGES: usize = 8192;

mod rep {
	use sc_peerset::ReputationChange as Rep;
	/// Reputation change when a peer sends us any message.
	///
	/// This forces node to verify it, thus the negative value here. Once message is verified,
	/// reputation change should be refunded with `ANY_MESSAGE_REFUND`.
	pub const ANY_MESSAGE: Rep = Rep::new(-(1 << 4), "Any message");
	/// Reputation change when a peer sends us any message that is not invalid.
	pub const ANY_MESSAGE_REFUND: Rep = Rep::new(1 << 4, "Any message (refund)");
	/// Reputation change when a peer sends us a message that we didn't know about.
	pub const GOOD_MESSAGE: Rep = Rep::new(1 << 7, "Good message");
	/// Reputation change when a peer sends us a bad message.
	pub const BAD_MESSAGE: Rep = Rep::new(-(1 << 12), "Bad message");
	/// We received an unexpected message packet.
	pub const UNEXPECTED_MESSAGE: Rep = Rep::new_fatal("Unexpected message packet");
}

/// Controls the behaviour of a [`GossipHandler`] it is connected to.
pub struct GossipHandlerController {
	to_handler: broadcast::Sender<ToHandler>,
	to_controller: broadcast::Sender<SignedDKGMessage<AuthorityId>>,
	gossip_enabled: Arc<AtomicBool>,
}

impl Clone for GossipHandlerController {
	fn clone(&self) -> Self {
		GossipHandlerController {
			to_handler: self.to_handler.clone(),
			to_controller: self.to_controller.clone(),
			gossip_enabled: self.gossip_enabled.clone(),
		}
	}
}

impl super::GossipEngineIface for GossipHandlerController {
	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		debug!(target: "dkg", "Sending message to {}", recipient);
		self.to_handler
			.send(ToHandler::SendMessage { recipient, message })
			.map(|_| ())
			.map_err(|_| DKGError::GenericError {
				reason: "Failed to send message to handler".into(),
			})
	}

	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		debug!(target: "dkg", "Sending message to all peers");
		self.to_handler.send(ToHandler::Gossip(message)).map(|_| ()).map_err(|_| {
			DKGError::GenericError { reason: "Failed to send message to handler".into() }
		})
	}

	fn stream(&self) -> Pin<Box<dyn Stream<Item = SignedDKGMessage<AuthorityId>> + Send>> {
		let stream = self.to_controller.subscribe();
		tokio_stream::wrappers::BroadcastStream::new(stream)
			.filter_map(|m| futures::future::ready(m.ok()))
			.boxed()
	}
}

#[derive(Clone, Debug)]
enum ToHandler {
	SendMessage { recipient: PeerId, message: SignedDKGMessage<AuthorityId> },
	Gossip(SignedDKGMessage<AuthorityId>),
}

impl GossipHandlerController {
	/// Controls whether messages are being gossiped on the network.
	pub fn set_gossip_enabled(&self, enabled: bool) {
		self.gossip_enabled.store(enabled, Ordering::Relaxed);
	}

	pub fn send_message(&self, recipient: PeerId, message: SignedDKGMessage<AuthorityId>) {
		self.to_handler.send(ToHandler::SendMessage { recipient, message }).unwrap();
	}

	pub fn gossip(&self, message: SignedDKGMessage<AuthorityId>) {
		self.to_handler.send(ToHandler::Gossip(message)).unwrap();
	}
}

/// Handler for gossiping messages. Call [`GossipHandler::run`] to start the processing.
pub struct GossipHandler<B: Block + 'static> {
	protocol_name: Cow<'static, str>,
	/// Pending Messages to be sent to the controller.
	to_controller: broadcast::Sender<SignedDKGMessage<AuthorityId>>,
	/// As multiple peers can send us the same message, we group
	/// these peers using the message hash while the message is
	/// received. This prevents that we receive the same message
	/// multiple times concurrently.
	pending_messages_peers: HashMap<B::Hash, Vec<PeerId>>,
	/// Network service to use to send messages and manage peers.
	service: Arc<NetworkService<B, B::Hash>>,
	/// Stream of networking events.
	event_stream: Pin<Box<dyn Stream<Item = Event> + Send>>,
	// All connected peers
	peers: HashMap<PeerId, Peer<B>>,
	// transaction_pool: Arc<dyn TransactionPool<H, B>>,
	gossip_enabled: Arc<AtomicBool>,
	from_controller: broadcast::Receiver<ToHandler>,
	to_handler: broadcast::Sender<ToHandler>,
	/// Prometheus metrics.
	metrics: Option<Metrics>,
}

/// Peer information
#[derive(Debug)]
struct Peer<B: Block> {
	/// Holds a set of messages known to this peer.
	known_messages: LruHashSet<B::Hash>,
}

impl<B: Block + 'static> GossipHandler<B> {
	/// Turns the [`GossipHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(mut self) {
		let stream = self.to_handler.subscribe();
		let mut incoming_messages = tokio_stream::wrappers::BroadcastStream::new(stream);
		debug!(target: "dkg", "Starting the DKG Gossip Handler");
		loop {
			futures::select! {
				network_event = self.event_stream.next().fuse() => {
					if let Some(network_event) = network_event {
						self.handle_network_event(network_event).await;
					} else {
						// Networking has seemingly closed. Closing as well.
						return;
					}
				},
				message = incoming_messages.next().fuse() => {
					match message {
						Some(Ok(ToHandler::SendMessage { recipient, message })) => self.send_signed_dkg_message(recipient, message),
						Some(Ok(ToHandler::Gossip(v))) => self.gossip_message(v),
						None => {
							// The broadcast stream has been closed.
							return;
						},
						_ => {},
					}
				},
			}
		}
	}

	async fn handle_network_event(&mut self, event: Event) {
		match event {
			Event::Dht(_) => {},
			Event::SyncConnected { remote } => {
				let addr = iter::once(multiaddr::Protocol::P2p(remote.into()))
					.collect::<multiaddr::Multiaddr>();
				let result = self.service.add_peers_to_reserved_set(
					self.protocol_name.clone(),
					iter::once(addr).collect(),
				);
				if let Err(err) = result {
					log::error!(target: "dkg-gossip", "Add reserved peer failed: {}", err);
				}
			},
			Event::SyncDisconnected { remote } => {
				self.service.remove_peers_from_reserved_set(
					self.protocol_name.clone(),
					iter::once(remote).collect(),
				);
			},

			Event::NotificationStreamOpened { remote, protocol, .. }
				if protocol == self.protocol_name =>
			{
				debug!(target: "dkg", "Peer {} connected to gossip protocol", remote);
				let _was_in = self.peers.insert(
					remote,
					Peer {
						known_messages: LruHashSet::new(
							NonZeroUsize::new(MAX_KNOWN_MESSAGES).expect("Constant is nonzero"),
						),
					},
				);
				debug_assert!(_was_in.is_none());
			},
			Event::NotificationStreamClosed { remote, protocol }
				if protocol == self.protocol_name =>
			{
				let _peer = self.peers.remove(&remote);
				debug!(target: "dkg", "Peer {} disconnected from gossip protocol", remote);
				debug_assert!(_peer.is_some());
			},

			Event::NotificationsReceived { remote, messages } => {
				for (protocol, message) in messages {
					if protocol != self.protocol_name {
						continue
					}
					debug!(target: "dkg", "Received message from {} from gossiping", remote);

					if let Ok(m) =
						<SignedDKGMessage<AuthorityId> as Decode>::decode(&mut message.as_ref())
					{
						self.on_signed_dkg_message(remote, m).await;
					} else {
						warn!(target: "dkg", "Failed to decode signed DKG message");
						self.service.report_peer(remote, rep::UNEXPECTED_MESSAGE);
					}
				}
			},
			Event::NotificationStreamOpened { .. } => {},
			Event::NotificationStreamClosed { .. } => {},
		}
	}

	/// Called when peer sends us new signed DKG message.
	async fn on_signed_dkg_message(&mut self, who: PeerId, message: SignedDKGMessage<AuthorityId>) {
		// Check behavior of the peer.
		// TODO: Fill in with proper check of message
		let some_check_here = false;
		if some_check_here {
			self.service.disconnect_peer(who, self.protocol_name.clone());
			self.service.report_peer(who, rep::UNEXPECTED_MESSAGE);
			return
		}

		// Accept messages only when enabled
		if !self.gossip_enabled.load(Ordering::Relaxed) {
			warn!(target: "dkg", "{} Ignoring dkg messages while disabled", who);
			return
		}

		debug!(target: "dkg", "Received a signed DKG messages from {}", who);
		if let Some(ref mut peer) = self.peers.get_mut(&who) {
			peer.known_messages.insert(message.message_hash::<B>());

			// self.service.report_peer(who, rep::ANY_MESSAGE);

			match self.pending_messages_peers.entry(message.message_hash::<B>()) {
				Entry::Vacant(entry) => {
					let _ = self.to_controller.send(message);
					entry.insert(vec![who]);
				},
				Entry::Occupied(mut entry) => {
					entry.get_mut().push(who);
				},
			}
		}
	}

	pub fn send_signed_dkg_message(
		&mut self,
		to_who: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) {
		let message_hash = message.message_hash::<B>();
		if let Some(ref mut peer) = self.peers.get_mut(&to_who) {
			let already_propagated = peer.known_messages.insert(message_hash.clone());
			if already_propagated {
				return
			}
			self.service.write_notification(
				to_who,
				self.protocol_name.clone(),
				Encode::encode(&message),
			);
			debug!(target: "dkg", "Sending a signed DKG messages to {}", to_who);
		} else {
			debug!(target: "dkg", "Peer {} does not exist in known peers", to_who);
		}
	}

	fn gossip_message(&mut self, message: SignedDKGMessage<AuthorityId>) {
		let mut propagated_messages = 0;
		let message_hash = message.message_hash::<B>();
		if self.peers.is_empty() {
			warn!(target: "dkg", "No peers to gossip message {}", message_hash);
		}
		for (who, peer) in self.peers.iter_mut() {
			let new_to_them = peer.known_messages.insert(message_hash.clone());
			if !new_to_them {
				continue
			}
			self.service.write_notification(
				*who,
				self.protocol_name.clone(),
				Encode::encode(&message),
			);
			propagated_messages += 1;
			debug!(target: "dkg", "Sending message to {}", who);
		}
		debug!(target: "dkg", "Gossiped {} messages", propagated_messages);
		if let Some(ref metrics) = self.metrics {
			metrics.propagated_messages.inc_by(propagated_messages as _)
		}
	}
}

/// Wrapper around `LinkedHashSet` with bounded growth.
///
/// In the limit, for each element inserted the oldest existing element will be removed.
#[derive(Debug, Clone)]
pub struct LruHashSet<T: Hash + Eq> {
	set: LinkedHashSet<T>,
	limit: NonZeroUsize,
}

impl<T: Hash + Eq> LruHashSet<T> {
	/// Create a new `LruHashSet` with the given (exclusive) limit.
	pub fn new(limit: NonZeroUsize) -> Self {
		Self { set: LinkedHashSet::new(), limit }
	}

	/// Insert element into the set.
	///
	/// Returns `true` if this is a new element to the set, `false` otherwise.
	/// Maintains the limit of the set by removing the oldest entry if necessary.
	/// Inserting the same element will update its LRU position.
	pub fn insert(&mut self, e: T) -> bool {
		if self.set.insert(e) {
			if self.set.len() == usize::from(self.limit) {
				self.set.pop_front(); // remove oldest entry
			}
			return true
		}
		false
	}
}
