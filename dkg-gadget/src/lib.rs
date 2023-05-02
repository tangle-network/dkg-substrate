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

use std::{marker::PhantomData, sync::Arc};

use debug_logger::DebugLogger;
use dkg_runtime_primitives::{crypto::AuthorityId, DKGApi, MaxAuthorities, MaxProposalLength};
use parking_lot::RwLock;
use prometheus::Registry;
use sc_client_api::{Backend, BlockchainEvents};
use sc_keystore::LocalKeystore;
use sc_network::{NetworkService, ProtocolName};
use sc_network_common::ExHashT;
use sp_api::{NumberFor, ProvideRuntimeApi};
use sp_blockchain::HeaderBackend;
use sp_keystore::SyncCryptoStorePtr;
use sp_runtime::traits::Block;

mod error;
/// Stores keypairs for DKG
pub mod keyring;
pub mod keystore;

pub mod gossip_engine;
mod signing_manager;
// mod meta_async_rounds;
pub mod db;
mod metrics;
mod proposal;
mod utils;
pub mod worker;

pub mod async_protocols;
pub mod debug_logger;
pub mod gossip_messages;
pub mod storage;

use gossip_engine::NetworkGossipEngineBuilder;
pub use keystore::DKGKeystore;

pub const DKG_KEYGEN_PROTOCOL_NAME: &str = "/webb-tools/dkg/keygen/1";
pub const DKG_SIGNING_PROTOCOL_NAME: &str = "/webb-tools/dkg/signing/1";

/// Returns the configuration value to put in
/// [`sc_network::config::NetworkConfiguration::extra_sets`].
pub fn dkg_peers_set_config(
	protocol_name: ProtocolName,
) -> sc_network_common::config::NonDefaultSetConfig {
	NetworkGossipEngineBuilder::set_config(protocol_name)
}

/// A convenience DKG client trait that defines all the type bounds a DKG client
/// has to satisfy. Ideally that should actually be a trait alias. Unfortunately as
/// of today, Rust does not allow a type alias to be used as a trait bound. Tracking
/// issue is <https://github.com/rust-lang/rust/issues/41517>.
pub trait Client<B, BE>:
	BlockchainEvents<B> + HeaderBackend<B> + ProvideRuntimeApi<B> + Send + Sync
where
	B: Block,
	BE: Backend<B>,
{
}

impl<B, BE, T> Client<B, BE> for T
where
	B: Block,
	BE: Backend<B>,
	T: BlockchainEvents<B> + HeaderBackend<B> + ProvideRuntimeApi<B> + Send + Sync,
{
	// empty
}

/// DKG gadget initialization parameters.
pub struct DKGParams<B, BE, C>
where
	B: Block,
	<B as Block>::Hash: ExHashT,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	/// DKG client
	pub client: Arc<C>,
	/// Client Backend
	pub backend: Arc<BE>,
	/// Synchronous key store pointer
	pub key_store: Option<SyncCryptoStorePtr>,
	/// Concrete local key store
	pub local_keystore: Option<Arc<LocalKeystore>>,
	/// Gossip network
	pub network: Arc<NetworkService<B, B::Hash>>,
	/// Prometheus metric registry
	pub prometheus_registry: Option<Registry>,
	/// For logging
	pub debug_logger: DebugLogger,
	/// Phantom block type
	pub _block: PhantomData<B>,
}

/// Start the DKG gadget.
///
/// This is a thin shim around running and awaiting a DKG worker.
pub async fn start_dkg_gadget<B, BE, C>(dkg_params: DKGParams<B, BE, C>)
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	// ensure logging-related statics are initialized
	dkg_logging::setup_log();

	let DKGParams {
		client,
		backend,
		key_store,
		network,
		prometheus_registry,
		local_keystore,
		_block,
		debug_logger,
	} = dkg_params;

	let dkg_keystore: DKGKeystore = DKGKeystore::new(key_store, debug_logger.clone());
	let keygen_gossip_protocol = NetworkGossipEngineBuilder::new(
		DKG_KEYGEN_PROTOCOL_NAME.to_string().into(),
		dkg_keystore.clone(),
	);

	let signing_gossip_protocol = NetworkGossipEngineBuilder::new(
		DKG_SIGNING_PROTOCOL_NAME.to_string().into(),
		dkg_keystore.clone(),
	);

	let logger_prometheus = debug_logger.clone();

	let metrics =
		prometheus_registry.as_ref().map(metrics::Metrics::register).and_then(
			|result| match result {
				Ok(metrics) => {
					logger_prometheus.debug("🕸️  Registered metrics");
					Some(metrics)
				},
				Err(err) => {
					logger_prometheus.debug(format!("🕸️  Failed to register metrics: {err:?}"));
					None
				},
			},
		);

	let latest_header = Arc::new(RwLock::new(None));

	let (keygen_gossip_handler, keygen_gossip_engine) = keygen_gossip_protocol
		.build(network.clone(), metrics.clone(), latest_header.clone(), debug_logger.clone())
		.expect("Keygen : Failed to build gossip engine");

	let (signing_gossip_handler, signing_gossip_engine) = signing_gossip_protocol
		.build(network.clone(), metrics.clone(), latest_header.clone(), debug_logger.clone())
		.expect("Signing : Failed to build gossip engine");

	// enable the gossip
	keygen_gossip_engine.set_gossip_enabled(true);
	signing_gossip_engine.set_gossip_enabled(true);

	keygen_gossip_engine.set_processing_already_seen_messages_enabled(false);
	signing_gossip_engine.set_processing_already_seen_messages_enabled(false);

	let keygen_handle = tokio::spawn(keygen_gossip_handler.run());
	let signing_handle = tokio::spawn(signing_gossip_handler.run());

	// In memory backend, not used for now
	// let db_backend = Arc::new(db::DKGInMemoryDb::new());
	let offchain_db_backend = db::DKGOffchainStorageDb::new(
		backend.clone(),
		dkg_keystore.clone(),
		local_keystore.clone(),
		debug_logger.clone(),
	);
	let db_backend = Arc::new(offchain_db_backend);
	let worker_params = worker::WorkerParams {
		latest_header,
		client,
		backend,
		key_store: dkg_keystore,
		keygen_gossip_engine,
		signing_gossip_engine,
		db_backend,
		metrics,
		local_keystore,
		network: Some(network),
		test_bundle: None,
		_marker: PhantomData::default(),
	};

	let worker = worker::DKGWorker::<_, _, _, _>::new(worker_params, debug_logger);
	deadlock_detection::deadlock_detect();

	worker.run().await;
	keygen_handle.abort();
	signing_handle.abort();
}

mod deadlock_detection {
	#[cfg(not(feature = "testing"))]
	pub fn deadlock_detect() {}

	#[cfg(feature = "testing")]
	pub fn deadlock_detect() {
		static HAS_STARTED: AtomicBool = AtomicBool::new(false);
		use parking_lot::deadlock;
		use std::{thread, time::Duration, sync::atomic::AtomicBool};

		// Create a background thread which checks for deadlocks every 10s
		thread::spawn(move || {
			if HAS_STARTED.compare_exchange(false, true, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst).unwrap_or(true) {
				println!("Deadlock detector already started");
				return
			}

			println!("Deadlock detector started");
			loop {
				thread::sleep(Duration::from_secs(5));
				let deadlocks = deadlock::check_deadlock();
				if deadlocks.is_empty() {
					continue
				}

				println!("{} deadlocks detected", deadlocks.len());
				for (i, threads) in deadlocks.iter().enumerate() {
					println!("Deadlock #{}", i);
					for t in threads {
						println!("Thread Id {:#?}", t.thread_id());
						println!("{:#?}", t.backtrace());
					}
				}
			}
		});
	}
}
