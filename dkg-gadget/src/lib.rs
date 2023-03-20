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

use dkg_logging::debug;
use parking_lot::RwLock;
use prometheus::Registry;

use sc_client_api::{Backend, BlockchainEvents};

use sc_network::{NetworkService, ProtocolName};
use sc_network_common::ExHashT;
use sp_api::{NumberFor, ProvideRuntimeApi};
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::Block;

use dkg_runtime_primitives::{crypto::AuthorityId, DKGApi};
use sc_keystore::LocalKeystore;
use sp_keystore::SyncCryptoStorePtr;

mod error;
mod keyring;
pub mod keystore;

mod gossip_engine;
// mod meta_async_rounds;
pub mod db;
mod metrics;
mod proposal;
#[cfg(feature = "testing")]
pub mod testing;
mod utils;
pub mod worker;

pub mod async_protocols;
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
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
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
	/// Phantom block type
	pub _block: PhantomData<B>,
}

/// Start the DKG gadget.
///
/// This is a thin shim around running and awaiting a DKG worker.
pub async fn start_dkg_gadget<B, BE, C>(dkg_params: DKGParams<B, BE, C>)
where
	B: Block,
	BE: Backend<B> + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
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
	} = dkg_params;
	let dkg_keystore: DKGKeystore = key_store.into();
	let keygen_gossip_protocol = NetworkGossipEngineBuilder::new(
		DKG_KEYGEN_PROTOCOL_NAME.to_string().into(),
		dkg_keystore.clone(),
	);

	let signing_gossip_protocol = NetworkGossipEngineBuilder::new(
		DKG_SIGNING_PROTOCOL_NAME.to_string().into(),
		dkg_keystore.clone(),
	);

	let metrics =
		prometheus_registry.as_ref().map(metrics::Metrics::register).and_then(
			|result| match result {
				Ok(metrics) => {
					debug!(target: "dkg_gadget", "🕸️  Registered metrics");
					Some(metrics)
				},
				Err(err) => {
					debug!(target: "dkg_gadget", "🕸️  Failed to register metrics: {:?}", err);
					None
				},
			},
		);

	let latest_header = Arc::new(RwLock::new(None));
	let (keygen_gossip_handler, keygen_gossip_engine) = keygen_gossip_protocol
		.build(network.clone(), metrics.clone(), latest_header.clone())
		.expect("Keygen : Failed to build gossip engine");

	let (signing_gossip_handler, signing_gossip_engine) = signing_gossip_protocol
		.build(network.clone(), metrics.clone(), latest_header.clone())
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
		_marker: PhantomData::default(),
	};

	let worker = worker::DKGWorker::<_, _, _, _>::new(worker_params);

	worker.run().await;
	keygen_handle.abort();
	signing_handle.abort();
}
