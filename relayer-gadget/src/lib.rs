//! Webb Relayer Gadget
//!
//! Integrates the Webb Relayer into the Substrate Node.

use sc_keystore::LocalKeystore;
use sp_keystore::SyncCryptoStorePtr;
use std::{path::PathBuf, sync::Arc};
use webb_relayer::service;
use webb_relayer_context::RelayerContext;

/// Webb Relayer gadget initialization parameters.
pub struct WebbRelayerParams {
	/// Synchronous key store pointer
	pub key_store: Option<SyncCryptoStorePtr>,
	/// Concrete local key store
	pub local_keystore: Option<Arc<LocalKeystore>>,
	/// Configuration directory
	pub config_dir: Option<PathBuf>,
	/// Database path
	pub database_path: Option<PathBuf>,
}

pub async fn start_relayer_gadget(relayer_params: WebbRelayerParams) {
	if relayer_params.config_dir.is_none() {
		tracing::error!(
			target: "relayer-gadget",
			"Not Starting Webb Relayer Gadget: No Config Directory Specified"
		);
		return
	}

	let config = webb_relayer_config::cli::load_config(relayer_params.config_dir)
		.expect("failed to load config");
	let store = create_store(relayer_params.database_path);
	let ctx = RelayerContext::new(config, store.clone()).expect("failed to build relayer context");

	// Start the web server:
	service::build_web_services(ctx.clone())
		.await
		.expect("failed to build relayer web services");
	service::ignite(ctx, Arc::new(store))
		.await
		.expect("failed to ignite relayer services");
}

/// Creates a database store for the relayer based on the configuration passed in.
pub fn create_store(database_path: Option<PathBuf>) -> webb_relayer_store::SledStore {
	let db_path = match database_path {
		Some(p) => p.join("relayerdb"),
		None => {
			tracing::debug!("Using temp dir for store");
			return webb_relayer_store::SledStore::temporary().expect("failed to create tmp store")
		},
	};

	webb_relayer_store::SledStore::open(db_path).expect("failed to open relayer store")
}
