//! Webb Relayer Gadget
//!
//! Integrates the Webb Relayer into the Substrate Node.

use dkg_runtime_primitives::crypto;
use ethereum_types::Secret;
use sc_keystore::LocalKeystore;
use sp_application_crypto::{ecdsa, ByteArray, CryptoTypePublicPair, Pair};
use sp_keystore::SyncCryptoStore;
use std::{path::PathBuf, sync::Arc};
use webb_relayer::service;
use webb_relayer_context::RelayerContext;

/// Webb Relayer gadget initialization parameters.
pub struct WebbRelayerParams {
	/// Concrete local key store
	pub local_keystore: Option<Arc<LocalKeystore>>,
	/// Configuration directory
	pub config_dir: Option<PathBuf>,
	/// Database path
	pub database_path: Option<PathBuf>,
}

pub async fn start_relayer_gadget(relayer_params: WebbRelayerParams) {
	let mut config = match relayer_params.config_dir {
		Some(p) => load_config(p),
		None => {
			tracing::error!(
				target: "relayer-gadget",
				"Error: Not Starting Webb Relayer Gadget. No Config Directory Specified"
			);
			return
		},
	};

	post_process_config(&mut config, relayer_params.local_keystore);

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

/// Loads the configuration from the given directory.
pub fn load_config<P>(config_dir: P) -> webb_relayer_config::WebbRelayerConfig
where
	P: AsRef<std::path::Path>,
{
	if !config_dir.as_ref().is_dir() {
		panic!("{} is not a directory", config_dir.as_ref().display());
	}
	webb_relayer_config::utils::load(config_dir).expect("failed to load relayer config")
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

/// Post process the relayer configuration.
///
/// Namely, if there is no signer for any EVM chain, set the signer to the ecdsa key from the
/// keystore.
/// Ensures that governance relayer is always enabled.
fn post_process_config(
	config: &mut webb_relayer_config::WebbRelayerConfig,
	local_key_store: Option<Arc<LocalKeystore>>,
) {
	let local_key_store = local_key_store.expect("failed to get local keystore");
	let ecdsa_public = local_key_store
		.keys(dkg_runtime_primitives::KEY_TYPE)
		.expect("failed to get keys")
		.into_iter()
		.find_map(|CryptoTypePublicPair(id, public_key)| {
			if id == ecdsa::CRYPTO_ID {
				crypto::Public::from_slice(&public_key).ok()
			} else {
				None
			}
		})
		.expect("failed to get ecdsa public key");
	let ecdsa_pair = local_key_store
		.key_pair::<crypto::Pair>(&ecdsa_public)
		.expect("failed to get ecdsa pair")
		.expect("failed to get ecdsa pair from local keystore");
	let ecdsa_secret = ecdsa_pair.to_raw_vec();
	// for each evm chain, if there is no signer, set the signer to the ecdsa key
	for chain in config.evm.values_mut() {
		if chain.private_key.is_none() {
			chain.private_key = Some(Secret::from_slice(&ecdsa_secret).into())
		}
	}
	// Make sure governance relayer is always enabled
	config.features.governance_relay = true;
}
