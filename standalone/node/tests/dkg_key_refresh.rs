#![cfg(unix)]
use assert_cmd::cargo::cargo_bin;
use nix::{
	sys::signal::{
		kill,
		Signal::{self, SIGINT, SIGTERM},
	},
	unistd::Pid,
};
use std::{
	convert::TryInto,
	io::Read,
	process::{self, Child, Command},
};
use tempfile::tempdir;

pub mod common;

#[tokio::test]
async fn dkg_key_refresh() {
	let mut alice = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
			"--dev",
			"--alice",
			"--tmp",
			"--port=30333",
			"--rpc-port=9933",
			"--ws-port=9945",
			"--node-key=0000000000000000000000000000000000000000000000000000000000000001",
			"--validator",
		])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	tokio::time::sleep(std::time::Duration::from_secs(10)).await;

	let mut bob = Command::new(cargo_bin("dkg-standalone-node"))
		.args(&[
            "--dev", 
            "--bob", 
            "--force-authoring", 
            "--tmp", 
            "--port=30334", 
            "--rpc-port=9934", 
            "--ws-port=9946", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", 
            "--validator"
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
            "--force-authoring", 
            "--tmp", 
            "--port=30335", 
            "--rpc-port=9935", 
            "--ws-port=9947", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", 
            "--validator"
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
            "--force-authoring", 
            "--tmp", 
            "--port=30336", 
            "--rpc-port=9936", 
            "--ws-port=9948", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", 
            "--validator"
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
            "--force-authoring", 
            "--tmp", 
            "--port=30337", 
            "--rpc-port=9937", 
            "--ws-port=9949", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", 
            "--validator"
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
            "--force-authoring", 
            "--tmp", 
            "--port=30338", 
            "--rpc-port=9938", 
            "--ws-port=9950", 
            "--bootnodes=/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", 
            "--validator"
        ])
		.stdout(process::Stdio::piped())
		.stderr(process::Stdio::piped())
		.stdin(process::Stdio::null())
		.spawn()
		.unwrap();

	let _ = common::wait_n_finalized_blocks(20, 360).await;

	assert!(alice.try_wait().unwrap().is_none(), "The Alice node should still be running");
	assert!(bob.try_wait().unwrap().is_none(), "The Bob node should still be running");
	assert!(charlie.try_wait().unwrap().is_none(), "The Charlie node should still be running");

	let mut alice_output_vec = vec![];
	let mut bob_output_vec = vec![];
	let mut charlie_output_vec = vec![];

	// Stop the processes
	kill(Pid::from_raw(alice.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(bob.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(charlie.id().try_into().unwrap()), SIGINT).unwrap();

	assert!(common::wait_for(&mut alice, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut bob, 40).map(|x| x.success()).unwrap_or_default());
	assert!(common::wait_for(&mut charlie, 40).map(|x| x.success()).unwrap_or_default());

	alice.stdout.as_mut().unwrap().read_to_end(&mut alice_output_vec).unwrap();
	bob.stdout.as_mut().unwrap().read_to_end(&mut bob_output_vec).unwrap();
	charlie.stdout.as_mut().unwrap().read_to_end(&mut charlie_output_vec).unwrap();

	// we examine the output, at least one worker should not submit an extrinsic
	println!("{}", String::from_utf8(alice_output_vec).unwrap());
	println!("{}", String::from_utf8(bob_output_vec).unwrap());
	println!("{}", String::from_utf8(charlie_output_vec).unwrap());
}
