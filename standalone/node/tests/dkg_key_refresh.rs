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
use std::{
	convert::TryInto,
	io::Write,
	process::{self, Command},
	str::FromStr,
};
use substrate_api_client::{rpc::WsRpcClient, Api, XtStatus};

pub mod common;

use common::{dkg_session_keys, get_account_id_from_seed, get_from_seed, set_keys};

pub fn get_node_url(port: u32) -> String {
	let node_ip = "ws://127.0.0.1";
	let url = format!("{}:{}", node_ip, port);
	url
}

#[tokio::test]
async fn dkg_key_refresh() {
	let mut alice = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
			"--dev",
			"--alice",
			"--port=30333",
			"--rpc-port=9933",
			"--ws-port=9945",
			"--node-key=0000000000000000000000000000000000000000000000000000000000000001",
		])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut bob = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--bob", 
            "--port=30334", 
            "--rpc-port=9934", 
            "--ws-port=9946", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut charlie = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--charlie", 
            "--port=30335", 
            "--rpc-port=9935", 
            "--ws-port=9947", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut dave = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--dave", 
            "--port=30336", 
            "--rpc-port=9936", 
            "--ws-port=9948", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut eve = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--eve", 
            "--port=30337", 
            "--rpc-port=9937", 
            "--ws-port=9949", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let mut ferdie = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--ferdie", 
            "--port=30338", 
            "--rpc-port=9938", 
            "--ws-port=9950", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	tokio::time::sleep(std::time::Duration::from_secs(10)).await;

	let alice_api = Api::new(WsRpcClient::new(&get_node_url(9945)))
		.map(|api| api.set_signer(AccountKeyring::Alice.pair()))
		.unwrap();
	let bob_api = Api::new(WsRpcClient::new(&get_node_url(9946)))
		.map(|api| api.set_signer(AccountKeyring::Bob.pair()))
		.unwrap();
	let charlie_api = Api::new(WsRpcClient::new(&get_node_url(9947)))
		.map(|api| api.set_signer(AccountKeyring::Charlie.pair()))
		.unwrap();
	let dave_api = Api::new(WsRpcClient::new(&get_node_url(9948)))
		.map(|api| api.set_signer(AccountKeyring::Dave.pair()))
		.unwrap();
	let eve_api = Api::new(WsRpcClient::new(&get_node_url(9949)))
		.map(|api| api.set_signer(AccountKeyring::Eve.pair()))
		.unwrap();
	let ferdie_api = Api::new(WsRpcClient::new(&get_node_url(9950)))
		.map(|api| api.set_signer(AccountKeyring::Ferdie.pair()))
		.unwrap();

	let mut terminal = Command::new("/bin/curl")
		.stdin(process::Stdio::piped())
		.stdout(process::Stdio::null())
		.spawn()
		.unwrap();
	let alice_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Alice").encode()));
	let bob_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Bob").encode()));
	let charlie_account = format!(
		"0x{}",
		hex::encode(get_account_id_from_seed::<sr25519::Public>("Charlie").encode())
	);
	let dave_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Dave").encode()));
	let eve_account =
		format!("0x{}", hex::encode(get_account_id_from_seed::<sr25519::Public>("Eve").encode()));
	let ferdie_account = format!(
		"0x{}",
		hex::encode(get_account_id_from_seed::<sr25519::Public>("Ferdie").encode())
	);

	let mut stdin = terminal.stdin.take().expect("failed to get stdin");

	let seed_and_pub_keys = vec![
		("alice", alice_account, 9933),
		("bob", bob_account, 9934),
		("charlie", charlie_account, 9935),
		("dave", dave_account, 9936),
		("eve", eve_account, 9937),
		("ferdie", ferdie_account, 9938),
	];

	let key_type = "_acc";

	for (seed, pub_key, port) in seed_and_pub_keys {
		stdin.write_all(&format!("-H 'Content-Type: application/json' --data '{{ \"jsonrpc\":\"2.0\", \"method\":\"author_insertKey\", \"params\":[\"{}\", \"{}\", \"{}\"],\"id\":1 }}' localhost:{}", key_type, seed, pub_key, port).encode()[..]).expect("failed to write to stdin");

		let session_keys = dkg_session_keys(
			get_from_seed::<GrandpaId>(seed),
			get_from_seed::<AuraId>(seed),
			get_from_seed::<DKGId>(seed),
		);

		match seed {
			"alice" => {
				let xt = set_keys(&alice_api, session_keys, vec![0,1,2,3]);

				    // send and watch extrinsic until finalized
					let tx_hash = alice_api
					.send_extrinsic(xt.hex_encode(), XtStatus::InBlock)
					.unwrap();
			},
			"bob" => {},
			"charlie" => {},
			"dave" => {},
			"eve" => {},
			"ferdie" => {},
		}
	}

	let _ = common::wait_n_finalized_blocks(20, 360).await;

	assert!(alice.try_wait().unwrap().is_none(), "The Alice node should still be running");
	assert!(bob.try_wait().unwrap().is_none(), "The Bob node should still be running");
	assert!(charlie.try_wait().unwrap().is_none(), "The Charlie node should still be running");
	assert!(dave.try_wait().unwrap().is_none(), "The Dave node should still be running");
	assert!(eve.try_wait().unwrap().is_none(), "The Eve node should still be running");
	assert!(ferdie.try_wait().unwrap().is_none(), "The Ferdie node should still be running");

	// Stop the processes
	kill(Pid::from_raw(alice.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(bob.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(charlie.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(dave.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(eve.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(ferdie.id().try_into().unwrap()), SIGINT).unwrap();

	assert!(common::wait_for(&mut alice, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut bob, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut charlie, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut dave, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut eve, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut ferdie, 40).map(|x| x.success()).unwrap_or_default());
}
