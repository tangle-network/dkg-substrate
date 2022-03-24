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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use dkg_primitives::rounds::DKGState;
use log::debug;
use prometheus::Registry;

use sc_client_api::{Backend, BlockchainEvents, Finalizer};
use sc_network_gossip::{GossipEngine, Network as GossipNetwork};

use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::{Block, Header};

use dkg_runtime_primitives::{crypto::AuthorityId, DKGApi};
use sc_keystore::LocalKeystore;
use sp_keystore::SyncCryptoStorePtr;

mod async_protocol_handlers;
mod error;
mod gossip;
mod keyring;
mod keystore;
pub mod messages;
mod metrics;
mod persistence;
mod proposal;
pub mod storage;
mod types;
mod utils;
mod worker;

pub use keystore::DKGKeystore;

pub const DKG_PROTOCOL_NAME: &str = "/webb-tools/dkg/1";

/// Returns the configuration value to put in
/// [`sc_network::config::NetworkConfiguration::extra_sets`].
pub fn dkg_peers_set_config() -> sc_network::config::NonDefaultSetConfig {
	let mut cfg =
		sc_network::config::NonDefaultSetConfig::new(DKG_PROTOCOL_NAME.into(), 1024 * 1024);
	cfg.allow_non_reserved(25, 25);
	cfg
}

/// A convenience DKG client trait that defines all the type bounds a DKG client
/// has to satisfy. Ideally that should actually be a trait alias. Unfortunately as
/// of today, Rust does not allow a type alias to be used as a trait bound. Tracking
/// issue is <https://github.com/rust-lang/rust/issues/41517>.
pub trait Client<B, BE>:
	BlockchainEvents<B> + HeaderBackend<B> + Finalizer<B, BE> + ProvideRuntimeApi<B> + Send + Sync
where
	B: Block,
	BE: Backend<B>,
{
}

impl<B, BE, T> Client<B, BE> for T
where
	B: Block,
	BE: Backend<B>,
	T: BlockchainEvents<B>
		+ HeaderBackend<B>
		+ Finalizer<B, BE>
		+ ProvideRuntimeApi<B>
		+ Send
		+ Sync,
{
	// empty
}

/// DKG gadget initialization parameters.
pub struct DKGParams<B, BE, C, N>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
	N: GossipNetwork<B> + Clone + Send + 'static,
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
	pub network: N,

	/// Prometheus metric registry
	pub prometheus_registry: Option<Registry>,
	/// Path to the persistent keystore directory for DKG data
	pub base_path: Option<PathBuf>,
	/// Phantom block type
	pub _block: std::marker::PhantomData<B>,
}

/// Start the DKG gadget.
///
/// This is a thin shim around running and awaiting a DKG worker.
pub async fn start_dkg_gadget<B, BE, C, N>(dkg_params: DKGParams<B, BE, C, N>)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
	N: GossipNetwork<B> + Clone + Send + 'static,
{
	let DKGParams {
		client,
		backend,
		key_store,
		network,
		prometheus_registry,
		base_path,
		local_keystore,
		_block,
	} = dkg_params;

	let gossip_validator = Arc::new(gossip::GossipValidator::new());
	let gossip_engine =
		GossipEngine::new(network, DKG_PROTOCOL_NAME, gossip_validator.clone(), None);

	let metrics =
		prometheus_registry.as_ref().map(metrics::Metrics::register).and_then(
			|result| match result {
				Ok(metrics) => {
					debug!(target: "dkg", "🕸️  Registered metrics");
					Some(metrics)
				},
				Err(err) => {
					debug!(target: "dkg", "🕸️  Failed to register metrics: {:?}", err);
					None
				},
			},
		);

	let worker_params = worker::WorkerParams {
		client,
		backend,
		key_store: key_store.into(),
		gossip_engine,
		metrics,
		base_path,
		local_keystore,
		dkg_state: DKGState {
			accepted: false,
			epoch_is_over: true,
			curr_dkg: None,
			past_dkg: None,
			listening_for_pub_key: false,
			listening_for_active_pub_key: false,
			created_offlinestage_at: HashMap::new(),
		},
	};

	let worker = worker::DKGWorker::<_, _, _>::new(worker_params);

	worker.run().await
}
