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
//!  - get a stream of DKG messages.
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
use crate::metrics::Metrics;
use codec::{Decode, Encode};
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{FutureExt, Stream, StreamExt};
use linked_hash_set::LinkedHashSet;
use log::{debug, warn};
use sc_network::{config, error, multiaddr, Event, NetworkService, PeerId};
use sp_runtime::traits::{Block, NumberFor};
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
use std::collections::HashSet;
use std::time::Instant;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use crate::meta_async_rounds::dkg_gossip_engine::ReceiveTimestamp;
use crate::worker::HasLatestHeader;

#[derive(Debug, Clone, Copy)]
pub struct NetworkGossipEngineBuilder;

impl NetworkGossipEngineBuilder {
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
		// 2. a channel to send DKG Messages back from the background task to the controller
		// (Background -> Controller).
		let (handler_channel, _) = broadcast::channel(MAX_PENDING_MESSAGES);
		let (controller_channel, _) = broadcast::channel(MAX_PENDING_MESSAGES);

		let gossip_enabled = Arc::new(AtomicBool::new(false));
		let rx_timestamps = Arc::new(RwLock::new(HashMap::new()));

		let handler = GossipHandler {
			latest_header,
			protocol_name: crate::DKG_PROTOCOL_NAME.into(),
			my_channel: handler_channel.clone(),
			rx_timestamps: rx_timestamps.clone(),
			controller_channel: controller_channel.clone(),
			pending_messages_peers: HashMap::new(),
			gossip_enabled: gossip_enabled.clone(),
			service,
			event_stream,
			peers: HashMap::new(),
			metrics,
		};

		let controller = GossipHandlerController {
			my_channel: controller_channel,
			handler_channel,
			gossip_enabled,
			rx_timestamps
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

#[allow(unused)]
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
#[derive(Clone)]
pub struct GossipHandlerController<B: Block> {
	/// a channel to send commands to the background task (Controller -> Background).
	handler_channel: broadcast::Sender<ToHandler>,
	/// a channel to send DKG Messages back from the background task to the controller
	///
	/// Technically, we do not need to hold a reference to this channel, but we do it to
	/// here to make this controller (**Clone-able**), meaning that we can clone it and
	/// still be able to receive messages from the background task.
	///
	/// Besides that, in the [`super::GossipEngineIface`] whenever we want to get the stream
	/// of the DKG messages (which requires the stream is Owned value), here
	/// we just create a new receiver of this channel, and since it is a broadcast channel,
	/// we can receive messages from all the clones of this controller.
	///
	/// See: [`GossipHandlerController::stream`] below.
	my_channel: broadcast::Sender<SignedDKGMessage<AuthorityId>>,
	/// Whether the gossip mechanism is enabled or not.
	gossip_enabled: Arc<AtomicBool>,
	rx_timestamps: ReceiveTimestamp<NumberFor<B>>
}

impl<B: Block> super::GossipEngineIface for GossipHandlerController<B> {
	type Clock = NumberFor<B>;

	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		debug!(target: "dkg", "Sending message to {}", recipient);
		self.handler_channel
			.send(ToHandler::SendMessage { recipient, message })
			.map(|_| ())
			.map_err(|_| DKGError::GenericError {
				reason: "Failed to send message to handler".into(),
			})
	}

	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		debug!(target: "dkg", "Sending message to all peers");
		self.handler_channel.send(ToHandler::Gossip(message)).map(|_| ()).map_err(|_| {
			DKGError::GenericError { reason: "Failed to send message to handler".into() }
		})
	}

	fn stream(&self) -> Pin<Box<dyn Stream<Item = SignedDKGMessage<AuthorityId>> + Send>> {
		// We need to create a new receiver of the channel, so that we can receive messages
		// from anywhere, without actually fight the rustc borrow checker.
		let stream = self.my_channel.subscribe();
		tokio_stream::wrappers::BroadcastStream::new(stream)
			.filter_map(|m| futures::future::ready(m.ok()))
			.boxed()
	}

	fn receive_timestamps(&self) -> Option<&ReceiveTimestamp<Self::Clock>> {
		Some(&self.rx_timestamps)
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
	/// Pending Messages to be sent to the [`GossipHandlerController`].
	controller_channel: broadcast::Sender<SignedDKGMessage<AuthorityId>>,
	/// As multiple peers can send us the same message, we group
	/// these peers using the message hash while the message is
	/// received. This prevents that we receive the same message
	/// multiple times concurrently.
	pending_messages_peers: HashMap<B::Hash, Vec<PeerId>>,
	/// Network service to use to send messages and manage peers.
	service: Arc<NetworkService<B, B::Hash>>,
	/// Stream of networking events.
	event_stream: Pin<Box<dyn Stream<Item = Event> + Send>>,
	/// A list of instants used to track participation frequency
	rx_timestamps: ReceiveTimestamp<NumberFor<B>>,
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
		B: Block
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
}

impl<B: Block + 'static> GossipHandler<B> {
	/// Turns the [`GossipHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(mut self) {
		let stream = self.my_channel.subscribe();
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
					HashSet::from([addr]),
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

		let now = self.get_latest_block_number();
		debug!(target: "dkg", "Received a signed DKG messages from {} @ {:?}", who, now);

		// TODO: consider the security of a user who manages to break through the underlying
		// libp2p crypto, and, spoofs a message with a false authority ID in order to trick
		// other nodes that some other node is misbehaving
		let inst = Instant::now();
		*self.rx_timestamps.write().entry(who).or_insert_with(||(now, inst, message.msg.id.clone())) = (now, inst, message.msg.id.clone());

		if let Some(ref mut peer) = self.peers.get_mut(&who) {
			peer.known_messages.insert(message.message_hash::<B>());

			// self.service.report_peer(who, rep::ANY_MESSAGE);

			match self.pending_messages_peers.entry(message.message_hash::<B>()) {
				Entry::Vacant(entry) => {
					let _ = self.controller_channel.send(message.clone());
					entry.insert(vec![who]);
				},
				Entry::Occupied(mut entry) => {
					entry.get_mut().push(who);
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
			let new_to_them = peer.known_messages.insert(message_hash);
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
