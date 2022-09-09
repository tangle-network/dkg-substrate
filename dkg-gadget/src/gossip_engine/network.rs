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

//! A DKG Gossip Engine that uses [`sc_network::NetworkService`] as a backend.
//!
//! In a nutshell, it works as follows:
//!
//! 1. You create a new [`NetworkGossipEngineBuilder`] which does not require any setup for now,
//! 2. You call [`NetworkGossipEngineBuilder::build`] to get two things:
//!   - a [`GossipHandler`] that is a simple background task that should run indefinitely, and
//!   - a [`GossipHandlerController`] that can be used to control that background task.
//!
//! The background task ([`GossipHandler`]) role is to listen, and gossip (if enabled) all the
//! DKG messages.
//!
//! From the [`GossipHandlerController`] which implements [`super::GossipEngineIface`], you can:
//!  - send a DKG message to a specific peer.
//!  - send a DKG message to all peers.
//!  - get a notification stream when you get a DKG message.
//!  - Have access to the message queue, which is a FIFO queue of DKG messages.
//!
//!
//! ### The Lifetime of the DKG Message:
//!
//! The DKG message is a [`SignedDKGMessage`] that is signed by the DKG authority, first it get
//! sent to the Gossip Engine either by calling [`GossipHandlerController::send`] or
//! [`GossipHandlerController::gossip`], depending on the call, the message will be sent to all
//! peers or only to a specific peer. on the other end, the DKG message is received by the DKG
//! engine, and it is verified then it will be added to the Engine's internal stream of DKG
//! messages, later the DKG Gadget will read this stream and process the DKG message.

use crate::{metrics::Metrics, worker::HasLatestHeader};
use codec::{Decode, Encode};
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{FutureExt, Stream, StreamExt};
use linked_hash_map::LinkedHashMap;
use log::{debug, warn};
use parking_lot::RwLock;
use sc_network::{config, error, multiaddr, Event, NetworkService, PeerId};
use sp_runtime::traits::{Block, NumberFor};
use std::{
	borrow::Cow,
	collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
	hash::Hash,
	iter,
	marker::PhantomData,
	num::NonZeroUsize,
	pin::Pin,
	sync::{
		atomic::{AtomicBool, AtomicUsize, Ordering},
		Arc,
	},
};
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct NetworkGossipEngineBuilder {
	protocol_name: Cow<'static, str>,
}

impl NetworkGossipEngineBuilder {
	/// Create a new network gossip engine.
	pub fn new(protocol_name: Cow<'static, str>) -> Self {
		Self { protocol_name }
	}

	/// Returns the configuration of the set to put in the network configuration.
	pub fn set_config(
		protocol_name: std::borrow::Cow<'static, str>,
	) -> config::NonDefaultSetConfig {
		config::NonDefaultSetConfig {
			notifications_protocol: protocol_name.clone(),
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

	/// Turns the builder into the actual handler. Returns a controller that allows controlling
	/// the behaviour of the handler while it's running.
	///
	/// Important: the gossip mechanism is initially disabled and doesn't gossip messages.
	/// You must call [`GossipHandlerController::set_gossip_enabled`] to enable it.
	///
	/// The returned values are:
	/// - a [`GossipHandler`] that is a simple background task that should run indefinitely, and
	/// - a [`GossipHandlerController`] that can be used to control that background task.
	pub(crate) fn build<B: Block>(
		self,
		service: Arc<NetworkService<B, B::Hash>>,
		metrics: Option<Metrics>,
		latest_header: Arc<RwLock<Option<B::Header>>>,
	) -> error::Result<(GossipHandler<B>, GossipHandlerController<B>)> {
		let event_stream = service.event_stream("dkg-handler").boxed();
		// Here we need to create few channels to communicate back and forth between the
		// background task and the controller.
		// since we have two things here we will need two channels:
		// 1. a channel to send commands to the background task (Controller -> Background).
		let (handler_channel, _) = broadcast::channel(MAX_PENDING_MESSAGES);
		let (message_notifications_channel, _) = broadcast::channel(MAX_PENDING_MESSAGES);
		let gossip_enabled = Arc::new(AtomicBool::new(false));
		let message_queue = Arc::new(RwLock::new(VecDeque::new()));
		let handler = GossipHandler {
			latest_header,
			protocol_name: self.protocol_name.clone(),
			my_channel: handler_channel.clone(),
			message_queue: message_queue.clone(),
			message_notifications_channel: message_notifications_channel.clone(),
			pending_messages_peers: HashMap::new(),
			gossip_enabled: gossip_enabled.clone(),
			service,
			event_stream,
			peers: HashMap::new(),
			metrics,
		};

		let controller = GossipHandlerController {
			handler_channel,
			message_notifications_channel,
			gossip_enabled,
			message_queue,
			_pd: Default::default(),
		};

		Ok((handler, controller))
	}
}

/// Maximum number of known messages hashes to keep for a peer.
const MAX_KNOWN_MESSAGES: usize = 10240; // ~300kb per peer + overhead.

/// Maximum allowed size for a DKG Signed Message notification.
const MAX_MESSAGE_SIZE: u64 = 16 * 1024 * 1024;

/// Maximum number of messages request we keep at any moment.
const MAX_PENDING_MESSAGES: usize = 8192;

/// Maximum number of duplicate messages that a single peer can send us.
///
/// This is to prevent a malicious peer from spamming us with messages.
const MAX_DUPLICATED_MESSAGES_PER_PEER: usize = 8;

#[allow(unused)]
mod rep {
	use sc_peerset::ReputationChange as Rep;
	/// Reputation change when a peer sends us a message that we didn't know about.
	pub const GOOD_MESSAGE: Rep = Rep::new(1 << 7, "Good message");
	/// We received an unexpected message packet.
	pub const UNEXPECTED_MESSAGE: Rep = Rep::new_fatal("Unexpected message packet");
	/// Reputation change when a peer sends us the same message over and over.
	pub const DUPLICATE_MESSAGE: Rep = Rep::new(-(1 << 12), "Duplicate message");
}

/// Controls the behaviour of a [`GossipHandler`] it is connected to.
#[derive(Clone)]
pub struct GossipHandlerController<B: Block> {
	/// a channel to send commands to the background task (Controller -> Background).
	handler_channel: broadcast::Sender<ToHandler>,
	/// A simple channel to send notifications whenever we receive a message from a peer.
	message_notifications_channel: broadcast::Sender<()>,
	/// A Buffer of messages that we have received from the network, but not yet processed.
	message_queue: Arc<RwLock<VecDeque<SignedDKGMessage<AuthorityId>>>>,
	/// Whether the gossip mechanism is enabled or not.
	gossip_enabled: Arc<AtomicBool>,
	/// Used to keep type information about the block. May
	/// be useful for the future, so keeping it here
	_pd: PhantomData<B>,
}

impl<B: Block> super::GossipEngineIface for GossipHandlerController<B> {
	type Clock = NumberFor<B>;

	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		debug!(target: "dkg_gadget::gossip_engine::network", "Sending message to {}", recipient);
		self.handler_channel
			.send(ToHandler::SendMessage { recipient, message })
			.map(|_| ())
			.map_err(|_| DKGError::GenericError {
				reason: "Failed to send message to handler".into(),
			})
	}

	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		debug!(target: "dkg_gadget::gossip_engine::network", "Sending message to all peers");
		self.handler_channel.send(ToHandler::Gossip(message)).map(|_| ()).map_err(|_| {
			DKGError::GenericError { reason: "Failed to send message to handler".into() }
		})
	}

	fn message_available_notification(&self) -> Pin<Box<dyn Stream<Item = ()> + Send>> {
		// We need to create a new receiver of the channel, so that we can receive messages
		// from anywhere, without actually fight the rustc borrow checker.
		let stream = self.message_notifications_channel.subscribe();
		tokio_stream::wrappers::BroadcastStream::new(stream)
			.filter_map(|m| futures::future::ready(m.ok()))
			.boxed()
	}

	fn peek_last_message(&self) -> Option<SignedDKGMessage<AuthorityId>> {
		let lock = self.message_queue.read();
		let msg = lock.front().cloned();
		match msg {
			Some(msg) => {
				log::debug!(target: "dkg_gadget::gossip_engine::network", "Dequeuing message: {}", msg.message_hash::<B>());
				Some(msg)
			},
			None => {
				log::debug!(target: "dkg_gadget::gossip_engine::network", "No message to dequeue");
				None
			},
		}
	}

	fn acknowledge_last_message(&self) {
		let mut lock = self.message_queue.write();
		let msg = lock.pop_front();
		match msg {
			Some(msg) => {
				log::debug!(target: "dkg_gadget::gossip_engine::network", "Acknowledging message: {}", msg.message_hash::<B>());
			},
			None => {
				log::debug!(target: "dkg_gadget::gossip_engine::network", "No message to acknowledge");
			},
		}
	}

	fn clear_queue(&self) {
		log::debug!(target: "dkg_gadget::gossip_engine::network", "Clearing message queue");
		let mut lock = self.message_queue.write();
		lock.clear();
	}
}
/// an Enum Representing the commands that can be sent to the background task.
#[derive(Clone, Debug)]
enum ToHandler {
	/// Send a DKG message to a peer.
	SendMessage { recipient: PeerId, message: SignedDKGMessage<AuthorityId> },
	/// Gossip a DKG message to all peers.
	Gossip(SignedDKGMessage<AuthorityId>),
}

impl<B: Block> GossipHandlerController<B> {
	/// Controls whether messages are being gossiped on the network.
	pub fn set_gossip_enabled(&self, enabled: bool) {
		self.gossip_enabled.store(enabled, Ordering::Relaxed);
	}
}

/// Handler for gossiping messages. Call [`GossipHandler::run`] to start the processing.
///
/// This is a background task that handles all the DKG messages.
pub struct GossipHandler<B: Block + 'static> {
	/// The Protocol Name, should be unique.
	///
	/// Used as an identifier for the gossip protocol.
	protocol_name: Cow<'static, str>,
	latest_header: Arc<RwLock<Option<B::Header>>>,
	/// A Buffer of messages that we have received from the network, but not yet processed.
	message_queue: Arc<RwLock<VecDeque<SignedDKGMessage<AuthorityId>>>>,
	/// A Simple notification stream to notify the caller that we have messages in the queue.
	message_notifications_channel: broadcast::Sender<()>,
	/// As multiple peers can send us the same message, we group
	/// these peers using the message hash while the message is
	/// received. This prevents that we receive the same message
	/// multiple times concurrently.
	pending_messages_peers: HashMap<B::Hash, HashSet<PeerId>>,
	/// Network service to use to send messages and manage peers.
	service: Arc<NetworkService<B, B::Hash>>,
	/// Stream of networking events.
	event_stream: Pin<Box<dyn Stream<Item = Event> + Send>>,
	// All connected peers
	peers: HashMap<PeerId, Peer<B>>,
	/// Whether the gossip mechanism is enabled or not.
	gossip_enabled: Arc<AtomicBool>,
	/// A Channel to receive commands from the controller.
	my_channel: broadcast::Sender<ToHandler>,
	/// Prometheus metrics.
	metrics: Option<Metrics>,
}

impl<B> HasLatestHeader<B> for GossipHandler<B>
where
	B: Block,
{
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}
}

/// Peer information
#[derive(Debug)]
struct Peer<B: Block> {
	/// Holds a set of messages known to this peer.
	known_messages: LruHashSet<B::Hash>,
	/// a counter of the messages that are received from this peer.
	///
	/// Implemented as a HashMap/LruHashMap with the message hash as the key,
	/// This is used to track the frequency of the messages received from this peer.
	/// If the same message is received from this peer more than
	/// `MAX_DUPLICATED_MESSAGES_PER_PEER`, we will flag this peer as malicious.
	message_counter: LruHashMap<B::Hash, usize>,
}

impl<B: Block + 'static> GossipHandler<B> {
	/// Turns the [`GossipHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(mut self) {
		let stream = self.my_channel.subscribe();
		let mut incoming_messages = tokio_stream::wrappers::BroadcastStream::new(stream);
		debug!(target: "dkg_gadget::gossip_engine::network", "Starting the DKG Gossip Handler");
		// a ticker timer to periodically check for new messages in the queue.
		let mut timer = tokio::time::interval(core::time::Duration::from_millis(50));
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
				_tick = timer.tick().fuse() => {
					// check if we have any messages in our queue.
					let read_lock = self.message_queue.read();
					if !read_lock.is_empty() {
						// notify the caller that we have messages in the queue.
						match self.message_notifications_channel.send(()) {
							Ok(_) => {},
							Err(_) => {
								// The receiver has been dropped, so we can't send any more messages.
								// Closing as well.
								log::debug!(target: "dkg_gadget::gossip_engine::network", "The message notification send has failed");
							},
						}
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
				let result = self
					.service
					.add_peers_to_reserved_set(self.protocol_name.clone(), HashSet::from([addr]));
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
				debug!(target: "dkg_gadget::gossip_engine::network", "Peer {} connected to gossip protocol", remote);
				let _was_in = self.peers.insert(
					remote,
					Peer {
						known_messages: LruHashSet::new(
							NonZeroUsize::new(MAX_KNOWN_MESSAGES).expect("Constant is nonzero"),
						),
						message_counter: LruHashMap::new(
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
				debug!(target: "dkg_gadget::gossip_engine::network", "Peer {} disconnected from gossip protocol", remote);
				debug_assert!(_peer.is_some());
			},

			Event::NotificationsReceived { remote, messages } => {
				for (protocol, message) in messages {
					if protocol != self.protocol_name {
						continue
					}
					debug!(target: "dkg_gadget::gossip_engine::network", "Received message from {} from gossiping", remote);

					if let Ok(m) =
						<SignedDKGMessage<AuthorityId> as Decode>::decode(&mut message.as_ref())
					{
						self.on_signed_dkg_message(remote, m).await;
					} else {
						warn!(target: "dkg_gadget::gossip_engine::network", "Failed to decode signed DKG message");
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
		let now = self.get_latest_block_number();
		debug!(target: "dkg", "{:?} round {:?} | Received a signed DKG messages from {} @ block {:?}, ", message.msg.status, message.msg.round_id, who, now);
		if let Some(ref mut peer) = self.peers.get_mut(&who) {
			peer.known_messages.insert(message.message_hash::<B>());
			match self.pending_messages_peers.entry(message.message_hash::<B>()) {
				Entry::Vacant(entry) => {
					log::debug!(target: "dkg_gadget::gossip_engine::network", "NEW DKG MESSAGE FROM {}", who);
					let mut queue_lock = self.message_queue.write();
					queue_lock.push_back(message.clone());
					drop(queue_lock);
					let recv_count = self.message_notifications_channel.receiver_count();
					if recv_count == 0 {
						log::warn!(target: "dkg", "No one is going to process the message notification!!!");
					}
					if let Err(e) = self.message_notifications_channel.send(()) {
						log::error!(target: "dkg", "Failed to send message notification to DKG controller: {:?}", e);
					} else {
						log::debug!(target: "dkg", "Message Notification sent to {recv_count} DKG controller listeners");
					}
					entry.insert(HashSet::from([who]));
					// This good, this peer is good, they sent us a message we didn't know about.
					// we should add some good reputation to them.
					self.service.report_peer(who, rep::GOOD_MESSAGE);
				},
				Entry::Occupied(mut entry) => {
					log::debug!(target: "dkg_gadget::gossip_engine::network", "OLD DKG MESSAGE FROM {}", who);
					// if we are here, that means this peer sent us a message we already know.
					let inserted = entry.get_mut().insert(who);
					// and if inserted is `false` that means this peer was already in the set
					// hence this not the first time we received this message from the exact same
					// peer.
					if !inserted {
						// we will increment the counter for this message.
						let old = peer
							.message_counter
							.get(&message.message_hash::<B>())
							.cloned()
							.unwrap_or(0);
						peer.message_counter.insert(message.message_hash::<B>(), old + 1);
						// and if we have received this message from the same peer more than
						// `MAX_DUPLICATED_MESSAGES_PER_PEER` times, we should report this peer
						// as malicious.
						if old >= MAX_DUPLICATED_MESSAGES_PER_PEER {
							self.service.report_peer(who, rep::DUPLICATE_MESSAGE);
						}
					}
				},
			}
		}

		// if the gossip is enabled, we send the message to the gossiping peers
		if self.gossip_enabled.load(Ordering::Relaxed) {
			self.gossip_message(message);
		}
	}

	pub fn send_signed_dkg_message(
		&mut self,
		to_who: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) {
		let message_hash = message.message_hash::<B>();
		if let Some(ref mut peer) = self.peers.get_mut(&to_who) {
			let already_propagated = peer.known_messages.insert(message_hash);
			if already_propagated {
				return
			}

			let notification_sender =
				self.service.notification_sender(to_who, self.protocol_name.clone());
			let tx = match notification_sender {
				Ok(tx) => tx,
				Err(e) => {
					// TODO(@shekohex): we should enqueue the message and try again later.
					warn!(target: "dkg_gadget::gossip_engine::network", "Failed to send notification to {}: {:?}", to_who, e);
					return
				},
			};

			let msg = Encode::encode(&message);
			let task = async move {
				match tx.ready().await {
					Ok(handle) => {
						let _ = handle.send(msg);
						debug!(target: "dkg_gadget::gossip_engine::network", "Sending a signed DKG messages to {}", to_who);
					},
					Err(e) => {
						warn!(target: "dkg_gadget::gossip_engine::network", "Failed to send notification to {:?}", e);
					},
				};
			};
			// FIXME(@shekohex): this is a temporary workaround, we should add timeouts for the
			// tasks.
			tokio::spawn(task);
		} else {
			debug!(target: "dkg_gadget::gossip_engine::network", "Peer {} does not exist in known peers", to_who);
		}
	}

	fn gossip_message(&mut self, message: SignedDKGMessage<AuthorityId>) {
		let propagated_messages = Arc::new(AtomicUsize::new(0));
		let message_hash = message.message_hash::<B>();
		if self.peers.is_empty() {
			warn!(target: "dkg_gadget::gossip_engine::network", "No peers to gossip message {}", message_hash);
		}
		let msg = Encode::encode(&message);
		for (who, peer) in self.peers.iter_mut() {
			let new_to_them = peer.known_messages.insert(message_hash);
			if !new_to_them {
				continue
			}
			let notification_sender =
				self.service.notification_sender(*who, self.protocol_name.clone());
			let tx = match notification_sender {
				Ok(tx) => tx,
				Err(e) => {
					// TODO(@shekohex): we should enqueue the message and try again later.
					warn!(target: "dkg_gadget::gossip_engine::network", "Failed to send notification to {}: {:?}", who, e);
					continue
				},
			};
			let local_propagated_messages = propagated_messages.clone();
			let msg = msg.clone();
			let task = async move {
				match tx.ready().await {
					Ok(handle) => {
						let _ = handle.send(msg);
						local_propagated_messages.fetch_add(1, Ordering::Relaxed);
					},
					Err(e) => {
						warn!(target: "dkg_gadget::gossip_engine::network", "Failed to send notification to {:?}", e);
					},
				};
			};
			// FIXME(@shekohex): this is a temporary workaround, we should add timeouts for the
			// tasks.
			tokio::spawn(task);
		}
		// FIXME(@shekohex): we should handle metrics at the end of each task, not in the function.
		if let Some(ref metrics) = self.metrics {
			metrics
				.propagated_messages
				.inc_by(propagated_messages.load(Ordering::Relaxed) as _);
		}
	}
}

/// Wrapper around `LinkedHashMap` with bounded growth.
///
/// In the limit, for each element inserted the oldest existing element will be removed.
#[derive(Debug, Clone)]
pub struct LruHashMap<K: Hash + Eq, V> {
	inner: LinkedHashMap<K, V>,
	limit: NonZeroUsize,
}

impl<K: Hash + Eq, V> LruHashMap<K, V> {
	/// Create a new `LruHashMap` with the given (exclusive) limit.
	pub fn new(limit: NonZeroUsize) -> Self {
		Self { inner: LinkedHashMap::new(), limit }
	}

	/// Insert element into the map.
	///
	/// Returns `true` if this is a new element to the map, `false` otherwise.
	/// Maintains the limit of the map by removing the oldest entry if necessary.
	/// Inserting the same element will update its LRU position.
	pub fn insert(&mut self, k: K, v: V) -> bool {
		if self.inner.insert(k, v).is_none() {
			if self.inner.len() == usize::from(self.limit) {
				// remove oldest entry
				self.inner.pop_front();
			}
			return true
		}
		false
	}

	/// Get an element from the map.
	/// Returns `None` if the element is not in the map.
	pub fn get(&self, k: &K) -> Option<&V> {
		self.inner.get(k)
	}
}

/// Wrapper around `LruHashMap` with bounded growth.
///
/// In the limit, for each element inserted the oldest existing element will be removed.
#[derive(Debug, Clone)]
pub struct LruHashSet<T: Hash + Eq> {
	set: LruHashMap<T, ()>,
}

impl<T: Hash + Eq> LruHashSet<T> {
	/// Create a new `LruHashSet` with the given (exclusive) limit.
	pub fn new(limit: NonZeroUsize) -> Self {
		Self { set: LruHashMap::new(limit) }
	}

	/// Insert element into the set.
	///
	/// Returns `true` if this is a new element to the set, `false` otherwise.
	/// Maintains the limit of the set by removing the oldest entry if necessary.
	/// Inserting the same element will update its LRU position.
	pub fn insert(&mut self, e: T) -> bool {
		self.set.insert(e, ())
	}
}
