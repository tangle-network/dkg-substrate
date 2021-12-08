#![cfg(unix)]
use assert_cmd::cargo::cargo_bin;
use codec::Encode;
use dkg_standalone_runtime::DKGId;
use nix::{
	sys::signal::{kill, Signal::SIGINT},
	unistd::Pid,
};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::sr25519;
use sp_finality_grandpa::AuthorityId as GrandpaId;
use sp_keyring::AccountKeyring;
use sp_runtime::MultiAddress;
use std::{
	convert::TryInto,
	io::Write,
	process::{self, Command},
	str::FromStr,
};
use substrate_api_client::{rpc::WsRpcClient, Api, XtStatus};

pub mod common;

use common::{
	dkg_session_keys, force_unstake, get_account_id_from_seed, get_from_seed, insert_key, set_keys,
};

pub fn get_node_url(port: u32) -> String {
	let node_ip = "ws://127.0.0.1";
	let url = format!("{}:{}", node_ip, port);
	url
}

#[tokio::test]
async fn dkg_key_refresh() {
	env_logger::init();
	let mut alice = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
			"--dev",
			"--alice",
			"--port=30333",
			"--rpc-port=9933",
			"--ws-port=45789",
			"--node-key=0000000000000000000000000000000000000000000000000000000000000001",
		])
		.stdout(process::Stdio::null())
		.stderr(process::Stdio::null())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut bob = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--bob", 
            "--port=30334", 
            "--rpc-port=9934", 
            "--ws-port=45790", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::null())
		.stderr(process::Stdio::null())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut charlie = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--charlie", 
            "--port=30335", 
            "--rpc-port=9935", 
            "--ws-port=45791", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::null())
		.stderr(process::Stdio::null())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	const MAX_TRIES: usize = 12;

	let alice_api = {
		let mut temp_api = None;
		for _ in 0..MAX_TRIES {
			let api = Api::new(WsRpcClient::new(&get_node_url(45789)))
				.map(|api| api.set_signer(AccountKeyring::Alice.pair()));

			if let Ok(api) = api {
				temp_api = Some(api);
				break
			} else {
				tokio::time::sleep(std::time::Duration::from_secs(20)).await;
			}
		}
		if let Some(temp_api) = temp_api {
			temp_api
		} else {
			panic!("Could not connect to the alice node")
		}
	};

	let bob_api = {
		let mut temp_api = None;
		for _ in 0..MAX_TRIES {
			let api = Api::new(WsRpcClient::new(&get_node_url(45790)))
				.map(|api| api.set_signer(AccountKeyring::Bob.pair()));

			if let Ok(api) = api {
				temp_api = Some(api);
				break
			} else {
				tokio::time::sleep(std::time::Duration::from_secs(20)).await;
			}
		}
		if let Some(temp_api) = temp_api {
			temp_api
		} else {
			panic!("Could not connect to the bob node")
		}
	};
	let charlie_api = {
		let mut temp_api = None;
		for _ in 0..MAX_TRIES {
			let api = Api::new(WsRpcClient::new(&get_node_url(45791)))
				.map(|api| api.set_signer(AccountKeyring::Charlie.pair()));

			if let Ok(api) = api {
				temp_api = Some(api);
				break
			} else {
				tokio::time::sleep(std::time::Duration::from_secs(20)).await;
			}
		}
		if let Some(temp_api) = temp_api {
			temp_api
		} else {
			panic!("Could not connect to the charlie node")
		}
	};

	let alice_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Alice").encode()));
	let bob_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Bob").encode()));
	let charlie_account = format!(
		"0x{}",
		hex::encode(get_account_id_from_seed::<sr25519::Public>("Charlie").encode())
	);

	let seed_and_pub_keys =
		vec![("Alice", alice_account), ("Bob", bob_account), ("Charlie", charlie_account)];

	let key_type = "_acc";

	for (seed, pub_key) in seed_and_pub_keys {
		// let session_keys = dkg_session_keys(
		// 	get_from_seed::<GrandpaId>(seed),
		// 	get_from_seed::<AuraId>(seed),
		// 	get_from_seed::<DKGId>(seed),
		// );

		match seed {
			"Alice" => {
				insert_key(&alice_api, key_type, &seed.to_lowercase(), &pub_key);
			},
			"Bob" => {
				insert_key(&bob_api, key_type, &seed.to_lowercase(), &pub_key);
			},
			"Charlie" => {
				insert_key(&charlie_api, key_type, &seed.to_lowercase(), &pub_key);
			},
			_ => {},
		}
	}



	let xt =
		force_unstake(&alice_api, MultiAddress::Id(AccountKeyring::Charlie.to_account_id()), 2);
	
	println!("[+] Composed Extrinsic:\n {:?}\n", xt);

	// send and watch extrinsic until broadcast
	let tx_hash = alice_api.send_extrinsic(xt.hex_encode(), XtStatus::Broadcast).unwrap();

	let _ = common::wait_n_finalized_blocks(35, 420).await;

	let next_key: Option<Vec<u8>> = alice_api.get_storage_value("DKG", "NextDKGPublicKey", None).unwrap();

	assert!(next_key.is_some(), "Next public key should be on chain by now");

	assert!(alice.try_wait().unwrap().is_none(), "The Alice node should still be running");
	assert!(bob.try_wait().unwrap().is_none(), "The Bob node should still be running");
	assert!(charlie.try_wait().unwrap().is_none(), "The Charlie node should still be running");

	// Stop the processes
	kill(Pid::from_raw(alice.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(bob.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(charlie.id().try_into().unwrap()), SIGINT).unwrap();

	assert!(common::wait_for(&mut alice, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut bob, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut charlie, 40).map(|x| x.success()).unwrap_or_default());
}
