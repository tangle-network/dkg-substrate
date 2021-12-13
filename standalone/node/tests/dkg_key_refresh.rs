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
#[macro_use]
extern crate lazy_static;
use std::{
	convert::TryInto,
	io::Write,
	process::{self, Command},
	str::FromStr,
	time::Duration,
};

use subxt::{ClientBuilder, PairSigner};

pub mod common;

use common::{get_pair_signer, node_runtime, spawn, Spawnable};

use std::path::PathBuf;

pub fn get_node_url(port: u32) -> String {
	let node_ip = "ws://127.0.0.1";
	let url = format!("{}:{}", node_ip, port);
	url
}

#[tokio::test]
async fn dkg_key_refresh() -> Result<(), subxt::Error> {
	env_logger::init();

	let (client, processes, ws_url) =
		spawn(vec![Spawnable::Alice, Spawnable::Charlie, Spawnable::Bob]).await
			.map_err(|e| subxt::Error::Other(e))?;

	let mut alice = processes[0];
	let mut charlie = processes[1];
	let mut bob = processes[2];

	let signer = PairSigner::<node_runtime::DefaultConfig, _>::new(AccountKeyring::Alice.pair());
	let charlie_acc = AccountKeyring::Charlie.to_account_id();
	let charlie_stash = get_pair_signer("Charlie//stash");

	let api = client.to_runtime_api::<node_runtime::RuntimeApi<node_runtime::DefaultConfig>>();

	let hash = api.tx().staking().chill().sign_and_submit(&charlie_stash).await?;

	println!("[+] Composed Extrinsic:\n {:?}\n", hash);

	let _ = common::wait_n_finalized_blocks(45, &ws_url, 540).await;

	let next_key: Option<(u64, Vec<u8>)> =
		api.storage().dkg().next_dkg_public_key(None).await?;

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
	Ok(())
}
