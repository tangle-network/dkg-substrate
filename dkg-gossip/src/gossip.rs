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

//! DKG gossip message handling to plug on top of the network service.
//!
//! Usage:
//!
//! - Use [`MessageHandlerPrototype::new`] to create a prototype.
//! - Pass the return value of [`MessageHandlerPrototype::set_config`] to the network
//! configuration as an extra peers set.
//! - Use [`MessageHandlerPrototype::build`] then [`MessageHandler::run`] to obtain a
//! `Future` that processes transactions.

use utils::{interval, LruHashSet};
use sc_network::{
	config::{self, TransactionImport, TransactionImportFuture, TransactionPool},
	error,
	Event, ExHashT, ObservedRole,
};

use sc_network::NetworkService;
use codec::{Decode, Encode, Codec};
use futures::{channel::mpsc, prelude::*, stream::FuturesUnordered};
use libp2p::{multiaddr, PeerId};
use log::{debug, trace, warn};
use prometheus_endpoint::{register, Counter, PrometheusError, Registry, U64};
use sp_runtime::{traits::Block as BlockT, app_crypto::sp_core::keccak_256};
use std::{
	borrow::Cow,
	collections::{hash_map::Entry, HashMap},
	iter,
	num::NonZeroUsize,
	pin::Pin,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	task::Poll,
	time, marker::PhantomData,
};
use sc_network::config::ProtocolId;

/// Interval at which we propagate transactions;
const PROPAGATE_TIMEOUT: time::Duration = time::Duration::from_millis(2900);

/// Maximum number of known transaction hashes to keep for a peer.
const MAX_KNOWN_MESSAGES: usize = 1024;

/// Maximum allowed size for a dkg message notification.
/// TODO: FIXME - This is a temporary value.
const MAX_MESSAGES_SIZE: u64 = 16 * 1024 * 1024;

/// Maximum number of message requests we keep at any moment.
/// TODO: FIXME - Potentially not needed, artifact from `transactions.rs`
const MAX_PENDING_MESSAGES: usize = 1024;

mod rep {
	use sc_peerset::ReputationChange as Rep;
	/// Reputation change when a peer sends us any message.
	///
	/// This forces node to verify it, thus the negative value here. Once transaction is verified,
	/// reputation change should be refunded with `ANY_MESSAGE_REFUND`
	pub const ANY_MESSAGE: Rep = Rep::new(-(1 << 4), "Any message");
	/// Reputation change when a peer sends us any message that is not invalid.
	pub const ANY_MESSAGE_REFUND: Rep = Rep::new(1 << 4, "Any message (refund)");
	/// Reputation change when a peer sends us an transaction that we didn't know about.
	pub const GOOD_MESSAGE: Rep = Rep::new(1 << 7, "Good transaction");
	/// Reputation change when a peer sends us a bad transaction.
	pub const BAD_MESSAGE: Rep = Rep::new(-(1 << 12), "Bad transaction");
	/// We received an unexpected transaction packet.
	pub const UNEXPECTED_MESSAGES: Rep = Rep::new_fatal("Unexpected transactions packet");
}

struct Metrics {
	propagated_messages: Counter<U64>,
}

impl Metrics {
	fn register(r: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			propagated_messages: register(
				Counter::new(
					"substrate_sync_propagated_messages",
					"Number of messages propagated to at least one peer",
				)?,
				r,
			)?,
		})
	}
}

#[pin_project::pin_project]
struct PendingMessage {
    // TODO: Decide if we want to use this pattern for validating messages
	#[pin]
	validation: TransactionImportFuture,
	msg: Vec<u8>,
}

impl Future for PendingMessage {
	type Output = (Vec<u8>, TransactionImport);

	fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
		let mut this = self.project();

		if let Poll::Ready(import_result) = Pin::new(&mut this.validation).poll_unpin(cx) {
			return Poll::Ready((this.msg.clone(), import_result))
		}

		Poll::Pending
	}
}

/// Prototype for a [`MessageHandler`].
pub struct MessageHandlerPrototype {
	protocol_name: Cow<'static, str>,
}

impl MessageHandlerPrototype {
	/// Create a new instance.
	pub fn new(protocol_id: ProtocolId) -> Self {
		Self { protocol_name: format!("/{}/transactions/1", protocol_id.as_ref()).into() }
	}

	/// Returns the configuration of the set to put in the network configuration.
	pub fn set_config(&self) -> config::NonDefaultSetConfig {
		config::NonDefaultSetConfig {
			notifications_protocol: self.protocol_name.clone(),
			fallback_names: Vec::new(),
			max_notification_size: MAX_MESSAGES_SIZE,
			set_config: config::SetConfig {
				in_peers: 0,
				out_peers: 0,
				reserved_nodes: Vec::new(),
				non_reserved_mode: config::NonReservedPeerMode::Deny,
			},
		}
	}

	/// Turns the prototype into the actual handler. Returns a controller that allows controlling
	/// the behaviour of the handler while it's running.
	///
	/// Important: the transactions handler is initially disabled and doesn't gossip transactions.
	/// You must call [`MessageHandlerController::set_gossip_enabled`] to enable it.
	pub fn build<B: BlockT + 'static, H: ExHashT, M>(
		self,
		service: Arc<NetworkService<B, H>>,
		local_role: config::Role,
        message_pool: Arc<dyn TransactionPool<H, B>>,
		metrics_registry: Option<&Registry>,
	) -> error::Result<(MessageHandler<B, H, M>, MessageHandlerController<H>)> {
		let event_stream = service.event_stream("transactions-handler").boxed();
		let (to_handler, from_controller) = mpsc::unbounded();
		let gossip_enabled = Arc::new(AtomicBool::new(false));

		let handler = MessageHandler {
			protocol_name: self.protocol_name,
			propagate_timeout: Box::pin(interval(PROPAGATE_TIMEOUT)),
			pending_messages: FuturesUnordered::new(),
			pending_message_peers: HashMap::new(),
			gossip_enabled: gossip_enabled.clone(),
			service,
			event_stream,
			peers: HashMap::new(),
            message_pool,
			local_role,
			from_controller,
			metrics: if let Some(r) = metrics_registry {
				Some(Metrics::register(r)?)
			} else {
				None
			},
            _phantom_message_type: PhantomData::new(),
		};

		let controller = MessageHandlerController { to_handler, gossip_enabled };

		Ok((handler, controller))
	}
}

/// Controls the behaviour of a [`MessageHandler`] it is connected to.
pub struct MessageHandlerController<H: ExHashT> {
	to_handler: mpsc::UnboundedSender<ToHandler<H>>,
	gossip_enabled: Arc<AtomicBool>,
}

impl<H: ExHashT> MessageHandlerController<H> {
	/// Controls whether transactions are being gossiped on the network.
	pub fn set_gossip_enabled(&mut self, enabled: bool) {
		self.gossip_enabled.store(enabled, Ordering::Relaxed);
	}

	/// You may call this when new transactions are imported by the transaction pool.
	///
	/// All transactions will be fetched from the `TransactionPool` that was passed at
	/// initialization as part of the configuration and propagated to peers.
	pub fn propagate_messages(&self) {
		let _ = self.to_handler.unbounded_send(ToHandler::PropagateMessages);
	}

	/// You must call when new a transaction is imported by the transaction pool.
	///
	/// This transaction will be fetched from the `TransactionPool` that was passed at
	/// initialization as part of the configuration and propagated to peers.
	pub fn propagate_message(&self, hash: H) {
		let _ = self.to_handler.unbounded_send(ToHandler::PropagateMessage(hash));
	}
}

enum ToHandler<H: ExHashT> {
	PropagateMessages,
	PropagateMessage(H),
}

/// Handler for transactions. Call [`MessageHandler::run`] to start the processing.
pub struct MessageHandler<B: BlockT + 'static, H: ExHashT, M> {
	protocol_name: Cow<'static, str>,
	/// Interval at which we call `propagate_messages`.
	propagate_timeout: Pin<Box<dyn Stream<Item = ()> + Send>>,
	/// Pending messages verification tasks.
	pending_messages: FuturesUnordered<PendingMessage>,
	/// As multiple peers can send us the same message, we group
	/// these peers using the message hash while the message is
	/// imported. This prevents that we import the same message
	/// multiple times concurrently.
	pending_message_peers: HashMap<[u8; 32], Vec<PeerId>>,
	/// Network service to use to send messages and manage peers.
	service: Arc<NetworkService<B, H>>,
	/// Stream of networking events.
	event_stream: Pin<Box<dyn Stream<Item = Event> + Send>>,
	// All connected peers
	peers: HashMap<PeerId, Peer<H>>,
    // TODO: Remove this and integrate meta handler or channel for polling incoming messages
	message_pool: Arc<dyn TransactionPool<H, B>>,
	gossip_enabled: Arc<AtomicBool>,
	local_role: config::Role,
	from_controller: mpsc::UnboundedReceiver<ToHandler<H>>,
	/// Prometheus metrics.
	metrics: Option<Metrics>,
    _phantom_message_type: PhantomData<M>,
}

/// Peer information
#[derive(Debug)]
struct Peer<H: ExHashT> {
	/// Holds a set of transactions known to this peer.
	known_messages: LruHashSet<H>,
	role: ObservedRole,
}

impl<B: BlockT + 'static, H: ExHashT, M: Codec> MessageHandler<B, H, M> {
	/// Turns the [`MessageHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(mut self) {
		loop {
			futures::select! {
				_ = self.propagate_timeout.next().fuse() => {
					self.propagate_messages();
				},
				(msg, result) = self.pending_messages.select_next_some() => {
					if let Some(peers) = self.pending_message_peers.remove(&msg) {
						peers.into_iter().for_each(|p| self.on_handle_transaction_import(p, result));
					} else {
						warn!(target: "sub-libp2p", "Inconsistent state, no peers for pending transaction!");
					}
				},
				network_event = self.event_stream.next().fuse() => {
					if let Some(network_event) = network_event {
						self.handle_network_event(network_event).await;
					} else {
						// Networking has seemingly closed. Closing as well.
						return;
					}
				},
				message = self.from_controller.select_next_some().fuse() => {
					match message {
						ToHandler::PropagateMessage(hash) => self.propagate_message(&hash),
						ToHandler::PropagateMessages => self.propagate_messages(),
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
					log::error!(target: "sync", "Add reserved peer failed: {}", err);
				}
			},
			Event::SyncDisconnected { remote } => {
				self.service.remove_peers_from_reserved_set(
					self.protocol_name.clone(),
					iter::once(remote).collect(),
				);
			},

			Event::NotificationStreamOpened { remote, protocol, role, .. }
			if protocol == self.protocol_name =>
				{
					let _was_in = self.peers.insert(
						remote,
						Peer {
							known_messages: LruHashSet::new(
								NonZeroUsize::new(MAX_KNOWN_MESSAGES).expect("Constant is nonzero"),
							),
							role,
						},
					);
					debug_assert!(_was_in.is_none());
				},
			Event::NotificationStreamClosed { remote, protocol }
			if protocol == self.protocol_name =>
				{
					let _peer = self.peers.remove(&remote);
					debug_assert!(_peer.is_some());
				},

			Event::NotificationsReceived { remote, messages } => {
				for (protocol, message) in messages {
					if protocol != self.protocol_name {
						continue
					}

					if let Ok(m) = <Vec<M> as Decode>::decode(
						&mut message.as_ref(),
					) {
						self.on_messages(remote, &m);
					} else {
						warn!(target: "sub-libp2p", "Failed to decode transactions list");
					}
				}
			},

			// Not our concern.
			Event::NotificationStreamOpened { .. } | Event::NotificationStreamClosed { .. } => {},
		}
	}

	/// Called when peer sends us new messages
	fn on_messages(&mut self, who: PeerId, messages: &[M]) {
		// sending messages to light node is considered a bad behavior
		if matches!(self.local_role, config::Role::Light) {
			debug!(target: "sync", "Peer {} is trying to send messages to the light node", who);
			self.service.disconnect_peer(who, self.protocol_name.clone());
			self.service.report_peer(who, rep::UNEXPECTED_MESSAGES);
			return
		}

		// Accept messages only when enabled
		if !self.gossip_enabled.load(Ordering::Relaxed) {
			trace!(target: "sync", "{} Ignoring messages while disabled", who);
			return
		}

		trace!(target: "sync", "Received {} messages from {}", messages.len(), who);
		if let Some(ref mut peer) = self.peers.get_mut(&who) {
			for m in messages {
				if self.pending_messages.len() > MAX_PENDING_MESSAGES {
					debug!(
						target: "sync",
						"Ignoring any further transactions that exceed `MAX_PENDING_MESSAGES`({}) limit",
						MAX_PENDING_MESSAGES,
					);
					break
				}

                let hash = keccak_256(&m.encode());
				peer.known_messages.insert(hash.clone());

				self.service.report_peer(who, rep::ANY_MESSAGE);

				match self.pending_message_peers.entry(hash.clone()) {
					Entry::Vacant(entry) => {
						self.pending_messages.push(PendingMessage {
							validation: self.message_pool.import(t),
							msg: m.encode(),
						});
						entry.insert(vec![who]);
					},
					Entry::Occupied(mut entry) => {
						entry.get_mut().push(who);
					},
				}
			}
		}
	}

	fn on_handle_transaction_import(&mut self, who: PeerId, import: TransactionImport) {
		match import {
			TransactionImport::KnownGood =>
				self.service.report_peer(who, rep::ANY_MESSAGE_REFUND),
			TransactionImport::NewGood => self.service.report_peer(who, rep::GOOD_TRANSACTION),
			TransactionImport::Bad => self.service.report_peer(who, rep::BAD_TRANSACTION),
			TransactionImport::None => {},
		}
	}

	/// Propagate one transaction.
	pub fn propagate_message(&mut self, hash: &H) {
		debug!(target: "sync", "Propagating transaction [{:?}]", hash);
		// Accept transactions only when enabled
		if !self.gossip_enabled.load(Ordering::Relaxed) {
			return
		}
		if let Some(transaction) = self.message_pool.transaction(hash) {
			let propagated_to = self.do_propagate_messages(&[(hash.clone(), transaction)]);
			self.message_pool.on_broadcasted(propagated_to);
		}
	}

	fn do_propagate_messages(
		&mut self,
		transactions: &[(H, B::Extrinsic)],
	) -> HashMap<H, Vec<String>> {
		let mut propagated_to = HashMap::<_, Vec<_>>::new();
		let mut propagated_messages = 0;

		for (who, peer) in self.peers.iter_mut() {
			// never send transactions to the light node
			if matches!(peer.role, ObservedRole::Light) {
				continue
			}

			let (hashes, to_send): (Vec<_>, Vec<_>) = transactions
				.iter()
				.filter(|&(ref hash, _)| peer.known_messages.insert(hash.clone()))
				.cloned()
				.unzip();

			propagated_messages += hashes.len();

			if !to_send.is_empty() {
				for hash in hashes {
					propagated_to.entry(hash).or_default().push(who.to_base58());
				}
				trace!(target: "sync", "Sending {} transactions to {}", to_send.len(), who);
				self.service
					.write_notification(*who, self.protocol_name.clone(), to_send.encode());
			}
		}

		if let Some(ref metrics) = self.metrics {
			metrics.propagated_messages.inc_by(propagated_messages as _)
		}

		propagated_to
	}

	/// Call when we must propagate ready transactions to peers.
	fn propagate_messages(&mut self) {
		// Accept transactions only when enabled
		if !self.gossip_enabled.load(Ordering::Relaxed) {
			return
		}
		debug!(target: "sync", "Propagating transactions");
		let transactions = self.message_pool.transactions();
		let propagated_to = self.do_propagate_messages(&transactions);
		self.message_pool.on_broadcasted(propagated_to);
	}
}
