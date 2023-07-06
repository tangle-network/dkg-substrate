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

use crate::{debug_logger::DebugLogger, metrics::Metrics, worker::HasLatestHeader, DKGKeystore};
use codec::{Decode, Encode};
use dkg_primitives::types::{DKGError, SignedDKGMessage};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::StreamExt;
use linked_hash_map::LinkedHashMap;
use parking_lot::{Mutex, RwLock};
use sc_network::{multiaddr, Event, NetworkService, NetworkStateInfo, PeerId, ProtocolName};
use sc_network_common::{
	config, error,
	service::{NetworkEventStream, NetworkNotification, NetworkPeers},
};
use sp_runtime::traits::{Block, NumberFor};
use std::{
	collections::{hash_map::Entry, HashMap, HashSet},
	hash::Hash,
	iter,
	marker::PhantomData,
	num::NonZeroUsize,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Clone)]
pub struct NetworkGossipEngineBuilder {
	protocol_name: ProtocolName,
	keystore: DKGKeystore,
}

impl NetworkGossipEngineBuilder {
	/// Create a new network gossip engine.
	pub fn new(protocol_name: ProtocolName, keystore: DKGKeystore) -> Self {
		Self { protocol_name, keystore }
	}

	/// Returns the configuration of the set to put in the network configuration.
	pub fn set_config(protocol_name: ProtocolName) -> config::NonDefaultSetConfig {
		config::NonDefaultSetConfig {
			handshake: None,
			notifications_protocol: protocol_name,
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
		logger: DebugLogger,
	) -> error::Result<(GossipHandler<B>, GossipHandlerController<B>)> {
		// Here we need to create few channels to communicate back and forth between the
		// background task and the controller.
		// since we have two things here we will need two channels:
		// 1. a channel to send commands to the background task (Controller -> Background).
		let (handler_channel, handler_channel_rx) = tokio::sync::mpsc::unbounded_channel();
		let (message_channel_tx, message_channel_rx) = tokio::sync::mpsc::unbounded_channel();
		let gossip_enabled = Arc::new(AtomicBool::new(false));
		let handler = GossipHandler {
			latest_header,
			keystore: self.keystore,
			protocol_name: self.protocol_name.clone(),
			to_receiver: message_channel_tx,
			incoming_messages_stream: Arc::new(Mutex::new(Some(handler_channel_rx))),
			pending_messages_peers: Arc::new(RwLock::new(HashMap::new())),
			authority_id_to_peer_id: Arc::new(RwLock::new(HashMap::new())),
			gossip_enabled: gossip_enabled.clone(),
			service,
			peers: Arc::new(RwLock::new(HashMap::new())),
			logger: logger.clone(),
			metrics: Arc::new(metrics),
		};

		let local_peer_id = handler.service.local_peer_id();

		let controller = GossipHandlerController {
			local_peer_id,
			protocol_name: self.protocol_name,
			handler_channel,
			message_notifications_channel: Arc::new(Mutex::new(Some(message_channel_rx))),
			gossip_enabled,
			logger,
			_pd: Default::default(),
		};

		Ok((handler, controller))
	}
}

/// Maximum number of known messages hashes to keep for a peer.
const MAX_KNOWN_MESSAGES: usize = 10240; // ~300kb per peer + overhead.

/// Maximum allowed size for a DKG Signed Message notification.
const MAX_MESSAGE_SIZE: u64 = 16 * 1024 * 1024;

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
	/// When a peer tries to impersonate another peer, by claiming another authority id.
	pub const PEER_IMPERSONATED: Rep = Rep::new_fatal("Peer is impersonating another peer");
	/// Reputation change when a peer sends us the same message over and over.
	pub const DUPLICATE_MESSAGE: Rep = Rep::new(-(1 << 12), "Duplicate message");
}

/// Controls the behaviour of a [`GossipHandler`] it is connected to.
#[derive(Clone)]
pub struct GossipHandlerController<B: Block> {
	local_peer_id: PeerId,
	protocol_name: ProtocolName,
	/// a channel to send commands to the background task (Controller -> Background).
	handler_channel: tokio::sync::mpsc::UnboundedSender<ToHandler>,
	/// where messages are received
	message_notifications_channel:
		Arc<Mutex<Option<UnboundedReceiver<SignedDKGMessage<AuthorityId>>>>>,
	/// Whether the gossip mechanism is enabled or not.
	gossip_enabled: Arc<AtomicBool>,
	logger: DebugLogger,
	/// Used to keep type information about the block. May
	/// be useful for the future, so keeping it here
	_pd: PhantomData<B>,
}

impl<B: Block> super::GossipEngineIface for GossipHandlerController<B> {
	type Clock = NumberFor<B>;

	fn logger(&self) -> &DebugLogger {
		&self.logger
	}

	fn local_peer_id(&self) -> PeerId {
		self.local_peer_id
	}

	fn send(
		&self,
		recipient: PeerId,
		message: SignedDKGMessage<AuthorityId>,
	) -> Result<(), DKGError> {
		self.logger.debug(format!(
			"Protocol : {:?} | Sending message to {}",
			self.protocol_name, recipient
		));
		self.handler_channel
			.send(ToHandler::SendMessage { recipient, message })
			.map_err(|_| DKGError::GenericError {
				reason: "Failed to send message to handler".into(),
			})
	}

	fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
		self.logger
			.debug(format!("Protocol : {:?} | Sending message to all peers", self.protocol_name));
		self.handler_channel.send(ToHandler::Gossip(message)).map(|_| ()).map_err(|_| {
			DKGError::GenericError { reason: "Failed to send message to handler".into() }
		})
	}

	fn get_stream(&self) -> Option<UnboundedReceiver<SignedDKGMessage<AuthorityId>>> {
		self.message_notifications_channel.lock().take()
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
	protocol_name: ProtocolName,
	latest_header: Arc<RwLock<Option<B::Header>>>,
	/// The DKG Keystore.
	keystore: DKGKeystore,
	/// A Simple notification stream to notify the caller that we have messages in the queue.
	to_receiver: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<AuthorityId>>,
	incoming_messages_stream: Arc<Mutex<Option<UnboundedReceiver<ToHandler>>>>,
	/// As multiple peers can send us the same message, we group
	/// these peers using the message hash while the message is
	/// received. This prevents that we receive the same message
	/// multiple times concurrently.
	pending_messages_peers: Arc<RwLock<HashMap<B::Hash, HashSet<PeerId>>>>,
	/// Network service to use to send messages and manage peers.
	service: Arc<NetworkService<B, B::Hash>>,
	// All connected peers
	peers: Arc<RwLock<HashMap<PeerId, Peer<B>>>>,
	/// A mapping from authority id to peer id.
	///
	/// This is used to send messages to specific peer by knowing the authority id.
	authority_id_to_peer_id: Arc<RwLock<HashMap<AuthorityId, PeerId>>>,
	/// Whether the gossip mechanism is enabled or not.
	gossip_enabled: Arc<AtomicBool>,
	logger: DebugLogger,
	/// Prometheus metrics.
	metrics: Arc<Option<Metrics>>,
}

impl<B: Block + 'static> Clone for GossipHandler<B> {
	fn clone(&self) -> Self {
		Self {
			protocol_name: self.protocol_name.clone(),
			latest_header: self.latest_header.clone(),
			keystore: self.keystore.clone(),
			to_receiver: self.to_receiver.clone(),
			incoming_messages_stream: self.incoming_messages_stream.clone(),
			pending_messages_peers: self.pending_messages_peers.clone(),
			service: self.service.clone(),
			peers: self.peers.clone(),
			authority_id_to_peer_id: self.authority_id_to_peer_id.clone(),
			gossip_enabled: self.gossip_enabled.clone(),
			logger: self.logger.clone(),
			metrics: self.metrics.clone(),
		}
	}
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

	/// The Known AuthorityId of that Peer.
	///
	/// Could be None if that peer did not handshake with us yet.
	authority_id: Option<AuthorityId>,
}

impl<B: Block + 'static> GossipHandler<B> {
	/// Turns the [`GossipHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(self) {
		let mut incoming_messages = self
			.incoming_messages_stream
			.lock()
			.take()
			.expect("incoming message stream already taken");
		let mut event_stream = self.service.event_stream("dkg-handler");
		self.logger.debug("Starting the DKG Gossip Handler");

		// we have two streams, one from the network and one from the controller.
		// hence we want to start two separate tasks, one for each stream that are running
		// in parallel, without blocking each other.

		// first task, handles the incoming messages/Commands from the controller.
		let self0 = self.clone();
		let incoming_messages_task =
			crate::utils::ExplicitPanicFuture::new(tokio::spawn(async move {
				while let Some(message) = incoming_messages.recv().await {
					match message {
						ToHandler::SendMessage { recipient, message } =>
							self0.send_signed_dkg_message(recipient, message),
						ToHandler::Gossip(v) => self0.gossip_dkg_signed_message(v),
					}
				}
			}));

		let self2 = self.clone();
		// second task, handles the incoming messages/events from the network stream.
		let network_events_task =
			crate::utils::ExplicitPanicFuture::new(tokio::spawn(async move {
				while let Some(event) = event_stream.next().await {
					self2.handle_network_event(event).await;
				}
			}));

		// wait for the first task to finish or error out.
		//
		// The reason why we wait for the first one to finish, is that it is a critical error
		// if any of them finished before the others, the must either run all together or none.
		//
		// Here is more information about the reason why we do this:
		// 1. if the network events task finished, it means that the network service has been
		//   dropped, which means that the node is shutting down, or a network issue has occurred,
		//   which is in this point a critical error.
		// 2. if the incoming messages task finished, it means that the controller has been dropped,
		//   which also could mean that there is now no communication between the controller and the
		//   handler task, which is a critical error.
		//   This could also mean that the node is shutting down, but in this case the network
		// events   task should have finished as well.
		// 3. The timer task, however, will never finish, unless the node is shutting down, in which
		//  case the network events task should have finished as well.
		let _result =
			futures::future::select_all(vec![network_events_task, incoming_messages_task]).await;
		self.logger.error("The DKG Gossip Handler has finished!!".to_string());
	}

	async fn handle_network_event(&self, event: Event) {
		match event {
			Event::Dht(_) => {},
			Event::SyncConnected { remote } => {
				let addr = iter::once(multiaddr::Protocol::P2p(remote.into()))
					.collect::<multiaddr::Multiaddr>();
				let result = self
					.service
					.add_peers_to_reserved_set(self.protocol_name.clone(), HashSet::from([addr]));
				if let Err(err) = result {
					self.logger.error(format!("Add reserved peer failed: {err}"));
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
				self.logger.debug(format!("Peer {remote} connected to gossip protocol"));
				// Send our Handshake message to that peer.
				if let Err(err) = self.send_handshake_message(remote).await {
					self.logger
						.error(format!("Send handshake message to peer {remote} failed: {err:?}"));
				} else {
					self.logger.debug("Send handshake message to peer {remote} succeeded");
				}
				let mut lock = self.peers.write();
				let _was_in = lock.insert(
					remote,
					Peer {
						known_messages: LruHashSet::new(
							NonZeroUsize::new(MAX_KNOWN_MESSAGES).expect("Constant is nonzero"),
						),
						message_counter: LruHashMap::new(
							NonZeroUsize::new(MAX_KNOWN_MESSAGES).expect("Constant is nonzero"),
						),
						// At this point we don't know the authority id of the peer, so we set it to
						// None. We will update it once we receive the handshake message from that
						// peer.
						authority_id: None,
					},
				);
				debug_assert!(_was_in.is_none());
			},
			Event::NotificationStreamClosed { remote, protocol }
				if protocol == self.protocol_name =>
			{
				let mut lock = self.peers.write();
				let peer = lock.remove(&remote);
				debug_assert!(peer.is_some());
				// remove that peer's authority id from the authority id to peer id map (if any).
				match peer {
					Some(peer) =>
						if let Some(authority_id) = peer.authority_id {
							let expected_remote =
								self.authority_id_to_peer_id.write().remove(&authority_id);
							// This should always be valid, if it isn't, it means that we have a bug
							// and somehow we have two peers with the same authority id.
							debug_assert_eq!(expected_remote, Some(remote));
						},
					None => {
						self.logger
							.debug("Peer {remote} disconnected without sending handshake message");
					},
				}
				self.logger.debug("Peer {remote} disconnected from gossip protocol");
			},

			Event::NotificationsReceived { remote, messages } => {
				for (protocol, message) in messages {
					if protocol != self.protocol_name {
						continue
					}
					self.logger.debug(format!("Received message from {remote} from gossiping"));
					let maybe_dkg_network_message =
						<super::DKGNetworkMessage as Decode>::decode(&mut message.as_ref());
					let m = match maybe_dkg_network_message {
						Ok(m) => m,
						Err(e) => {
							self.logger.warn(format!("Failed to decode DKG Network message from peer {remote} with error: {e:?}"));
							self.service.report_peer(remote, rep::UNEXPECTED_MESSAGE);
							return
						},
					};
					match m {
						super::DKGNetworkMessage::Handshake(h) =>
							self.on_handshake_message(remote, h).await,
						super::DKGNetworkMessage::DKGMessage(s) =>
							self.on_signed_dkg_message(remote, s).await,
					};
				}
			},
			Event::NotificationStreamOpened { .. } => {},
			Event::NotificationStreamClosed { .. } => {},
		}
	}

	/// Creates and sends handshake message to the peer.
	async fn send_handshake_message(&self, to_who: PeerId) -> Result<(), DKGError> {
		let my_peer_id = self.service.local_peer_id();
		let handshake_message = super::HandshakeMessage::try_new(&self.keystore, my_peer_id)?;
		let message = super::DKGNetworkMessage::Handshake(handshake_message);
		let msg = Encode::encode(&message);
		self.service.write_notification(to_who, self.protocol_name.clone(), msg);
		Ok(())
	}

	async fn on_handshake_message(&self, who: PeerId, message: super::HandshakeMessage) {
		// verifiy the handshake message
		match message.is_valid(who) {
			Ok(v) =>
				if v {
					self.logger.debug(format!("Handshake message from peer {who} is valid"));
				} else {
					self.logger.warn(format!("Handshake message from peer {who} is invalid"));
					self.service.report_peer(who, rep::PEER_IMPERSONATED);
				},
			Err(e) => {
				self.logger.warn(format!(
					"Failed to verify handshake message from peer {who} with error: {e:?}"
				));
				self.service.report_peer(who, rep::UNEXPECTED_MESSAGE);
			},
		};
		self.logger
			.debug(format!("Peer {who} is now connected as {}", message.authority_id));
		let mut lock = self.peers.write();
		if let Some(peer) = lock.get_mut(&who) {
			peer.authority_id = Some(message.authority_id.clone());
		} else {
			self.logger
				.warn(format!("Peer {who} is not connected, but sent us a handshake message!!"));
		}
		self.authority_id_to_peer_id.write().insert(message.authority_id, who);
	}

	/// Called when peer sends us new signed DKG message.
	async fn on_signed_dkg_message(&self, who: PeerId, message: SignedDKGMessage<AuthorityId>) {
		// Check behavior of the peer.
		let now = self.get_latest_block_number();
		self.logger.debug(format!(
			"session {:?} | Received a signed DKG messages from {} @ block {:?}, ",
			message.msg.session_id, who, now
		));

		if let Some(metrics) = self.metrics.as_ref() {
			metrics.dkg_signed_messages.inc();
		}

		if let Some(ref mut peer) = self.peers.write().get_mut(&who) {
			peer.known_messages.insert(message.message_hash::<B>());
			let mut pending_messages_peers = self.pending_messages_peers.write();
			let send_the_message = |message: SignedDKGMessage<AuthorityId>| {
				if let Err(e) = self.to_receiver.send(message) {
					self.logger.error(format!(
						"Failed to send message notification to DKG controller: {e:?}"
					));
				} else {
					self.logger.debug(
						"Message Notification sent to {recv_count} DKG controller listeners",
					);
				}
			};
			match pending_messages_peers.entry(message.message_hash::<B>()) {
				Entry::Vacant(entry) => {
					self.logger.debug(format!("NEW DKG MESSAGE FROM {who}"));
					if let Some(metrics) = self.metrics.as_ref() {
						metrics.dkg_new_signed_messages.inc();
					}
					send_the_message(message.clone());
					entry.insert(HashSet::from([who]));
					// This good, this peer is good, they sent us a message we didn't know about.
					// we should add some good reputation to them.
					self.service.report_peer(who, rep::GOOD_MESSAGE);
				},
				Entry::Occupied(mut entry) => {
					self.logger.debug(format!("OLD DKG MESSAGE FROM {who}"));
					if let Some(metrics) = self.metrics.as_ref() {
						metrics.dkg_old_signed_messages.inc();
					}
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
							self.logger.warn(format!(
								"Reporting peer {who} as they are sending us the same message over and over again"
							));
							self.service.report_peer(who, rep::DUPLICATE_MESSAGE);
						}
					}

					// send the old message anyways
					send_the_message(message.clone());
				},
			}
		}

		// if the gossip is enabled, we send the message to the gossiping peers
		if self.gossip_enabled.load(Ordering::Relaxed) {
			self.gossip_dkg_signed_message(message);
		}
	}

	pub fn send_signed_dkg_message(&self, to_who: PeerId, message: SignedDKGMessage<AuthorityId>) {
		let message_hash = message.message_hash::<B>();
		if let Some(ref mut peer) = self.peers.write().get_mut(&to_who) {
			let new_to_them = peer.known_messages.insert(message_hash);
			if !new_to_them {
				return
			}
			let message = super::DKGNetworkMessage::DKGMessage(message);
			let msg = Encode::encode(&message);
			self.service.write_notification(to_who, self.protocol_name.clone(), msg);

			if let Some(metrics) = self.metrics.as_ref() {
				metrics.dkg_propagated_messages.inc();
			}
		} else {
			self.logger.debug(format!("Peer {to_who} does not exist in known peers"));
		}
	}

	fn gossip_dkg_signed_message(&self, message: SignedDKGMessage<AuthorityId>) {
		// Check if the message has a recipient
		let maybe_peer_id = match &message.msg.recipient_id {
			Some(recipient_id) => self.authority_id_to_peer_id.read().get(recipient_id).cloned(),
			None => None,
		};
		// If we have a peer id, we send the message to that peer directly.
		if let Some(peer_id) = maybe_peer_id {
			self.logger.debug(format!("Sending message to recipient {peer_id} using p2p"));
			self.send_signed_dkg_message(peer_id, message.clone());
		// potential bug "fix"
		//return
		} else if let Some(recipient_id) = &message.msg.recipient_id {
			self.logger.debug(format!(
				"No direct connection to {recipient_id}, falling back to gossiping"
			));
		} else {
			self.logger
				.debug("No specific recipient, broadcasting message to all peers".to_string());
		}
		// Otherwise, we fallback to sending the message to all peers.
		let peer_ids = {
			let peers_map = self.peers.read();
			peers_map.keys().cloned().collect::<Vec<_>>()
		};
		if peer_ids.is_empty() {
			let message_hash = message.message_hash::<B>();
			self.logger.warn(format!("No peers to gossip message {message_hash}"));
			return
		}
		for peer in peer_ids {
			self.send_signed_dkg_message(peer, message.clone());
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
