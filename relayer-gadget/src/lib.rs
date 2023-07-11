//! Webb Relayer Gadget
//!
//! Integrates the Webb Relayer into the Substrate Node.

use sc_keystore::LocalKeystore;
use sp_keystore::SyncCryptoStorePtr;
use std::sync::Arc;
use webb_relayer::service;
use webb_relayer_context::RelayerContext;

/// Webb Relayer gadget initialization parameters.
pub struct WebbRelayerParams {
	/// Synchronous key store pointer
	pub key_store: Option<SyncCryptoStorePtr>,
	/// Concrete local key store
	pub local_keystore: Option<Arc<LocalKeystore>>,
}

pub async fn start_relayer_gadget(relayer_params: WebbRelayerParams) {
	let config = webb_relayer_config::utils::load("path/to/config/directory")
		.expect("failed to load config");
	// next is to build the store, or the storage backend:
	let store =
		webb_relayer_store::sled::SledStore::open("path/to/store").expect("failed to open store");

	// finally, after loading the config files, we can build the relayer context.
	let ctx = RelayerContext::new(config, store.clone()).expect("failed to build relayer context");

	// it is now up to you to start the web interface/server for the relayer and the background
	// services.

	// Start the web server:
	service::build_web_services(ctx.clone())
		.await
		.expect("failed to build relayer web services");

	// and also the background services:
	// this does not block, will fire the services on background tasks.
	service::ignite(ctx, Arc::new(store))
		.await
		.expect("failed to ignite relayer services");
}
