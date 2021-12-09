use codec::Encode;
use sc_service::{ChainType, Configuration};
use sp_core::{sr25519, Pair, Public};
use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};
use sp_runtime::key_types::ACCOUNT;

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

pub fn insert_controller_account_keys_into_keystore(
	config: &Configuration,
	key_store: Option<SyncCryptoStorePtr>,
) {
	let chain_type = config.chain_spec.chain_type();
	let seed = &config.network.node_name[..];

	match seed {
		// When running the chain in dev or local test net, we insert the sr25519 account keys for collator accounts or validator accounts into the keystore
		// Only if the node running is one of the predefined nodes Alice, Bob, Charlie, Dave, Eve or Ferdie
		"Alice" | "Bob" | "Charlie" | "Dave" | "Eve" | "Ferdie" => {
			if chain_type == ChainType::Development || chain_type == ChainType::Local {
				let pub_key = get_from_seed::<sr25519::Public>(&seed).encode();
				if let Some(keystore) = key_store {
					let _ = SyncCryptoStore::insert_unknown(
						&*keystore,
						ACCOUNT,
						&format!("//{}", seed),
						&pub_key,
					);
				}
			}
		},
		_ => {},
	}
}
