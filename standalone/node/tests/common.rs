// This file is part of Substrate.

// Copyright (C) 2020-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg(unix)]

use dkg_standalone_runtime::{AccountId, DKGId, Signature};

use node_primitives::Block;
use remote_externalities::rpc_api;
use serde::Serialize;
use serde_json::{json, to_value, Value};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{sr25519, Pair, Public};
use sp_finality_grandpa::AuthorityId as GrandpaId;
use sp_runtime::traits::{IdentifyAccount, Verify};
use std::{
	thread,
	net::TcpListener,
	process::{self, Child, ExitStatus},
	sync::atomic::{AtomicU16, Ordering},
	time::Duration,
};
use subxt::{Client, ClientBuilder, PairSigner};
use tokio::time::timeout;
use lazy_static;

use std::path::PathBuf;

#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod node_runtime {}

static LOCALHOST_WS: &str = "ws://127.0.0.1:45789/";

/// Generate a crypto pair from seed.
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

pub fn get_pair_signer(seed: &str) -> PairSigner<node_runtime::DefaultConfig, sr25519::Pair> {
	PairSigner::<node_runtime::DefaultConfig, sr25519::Pair>::new(
		sr25519::Pair::from_string(&format!("//{}", seed), None)
			.expect("static values are known good; qed"),
	)
}

type AccountPublic = <Signature as Verify>::Signer;

/// Generate an account ID from seed.
pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId
where
	AccountPublic: From<<TPublic::Pair as Pair>::Public>,
{
	AccountPublic::from(get_from_seed::<TPublic>(seed)).into_account()
}

/// Wait for at least n blocks to be finalized within a specified time.
pub async fn wait_n_finalized_blocks(
	n: usize,
	url: &str,
	timeout_secs: u64,
) -> Result<(), tokio::time::error::Elapsed> {
	timeout(Duration::from_secs(timeout_secs), wait_n_finalized_blocks_from(n, url)).await
}

/// Wait for the given `child` the given number of `secs`.
///
/// Returns the `Some(exit status)` or `None` if the process did not finish in the given time.
pub fn wait_for(child: &mut Child, secs: u64) -> Result<ExitStatus, ()> {
	let result = wait_timeout::ChildExt::wait_timeout(child, Duration::from_secs(5.min(secs)))
		.map_err(|_| ())?;
	if let Some(exit_status) = result {
		Ok(exit_status)
	} else {
		if secs > 5 {
			eprintln!("Child process taking over 5 seconds to exit gracefully");
			let result = wait_timeout::ChildExt::wait_timeout(child, Duration::from_secs(secs - 5))
				.map_err(|_| ())?;
			if let Some(exit_status) = result {
				return Ok(exit_status)
			}
		}
		eprintln!("Took too long to exit (> {} seconds). Killing...", secs);
		let _ = child.kill();
		child.wait().unwrap();
		Err(())
	}
}

/// Wait for at least n blocks to be finalized from a specified node
pub async fn wait_n_finalized_blocks_from(n: usize, url: &str) {
	let mut built_blocks = std::collections::HashSet::new();
	let mut interval = tokio::time::interval(Duration::from_secs(2));

	loop {
		if let Ok(block) = rpc_api::get_finalized_head::<Block, _>(url.to_string()).await {
			built_blocks.insert(block);
			if built_blocks.len() > n {
				break
			}
		};
		interval.tick().await;
	}
}

lazy_static! {
	static ref BIN_PATH: PathBuf = assert_cmd::cargo::cargo_bin("dkg-standalone-node");
}

#[derive(PartialEq)]
pub enum Spawnable {
	Alice,
	Charlie,
	Bob,
	Dave,
	Ferdie,
	Eve,
}

/// Spawn the dkg nodes at the given path, and wait for rpc to be initialized.
/// The Alice node should be the first value in the list
pub async fn spawn(
	nodes: Vec<Spawnable>,
) -> Result<(subxt::Client<node_runtime::DefaultConfig>, Vec<Child>, String), String> {
	assert!(nodes[0] == Spawnable::Alice, "Alice should be the first value");
	let mut port = 9944u16;

	let mut processes = Vec::new();

	for node in nodes {
		let mut node_name = "alice";
		match node {
			Spawnable::Alice => {},
			Spawnable::Charlie => node_name = "charlie",
			Spawnable::Bob => node_name = "bob",
			Spawnable::Dave => node_name = "dave",
			Spawnable::Ferdie => node_name = "ferdie",
			Spawnable::Eve => node_name = "eve",
		}

		let mut cmd = process::Command::new(&*BIN_PATH);
		cmd.env("RUST_LOG", "error").arg(format!("--{}", node_name));

		let (p2p_port, http_port, ws_port) = next_open_port()
			.ok_or_else(|| "No available ports in the given port range".to_owned())?;

		cmd.arg(format!("--port={}", p2p_port));
		cmd.arg(format!("--rpc-port={}", http_port));
		cmd.arg(format!("--ws-port={}", ws_port));

		if node_name == "alice" {
			cmd.arg("--node-key=0000000000000000000000000000000000000000000000000000000000000001");
			port = ws_port;
		} else {
			cmd.arg(format!("--bootnodes=/ip4/127.0.0.1/tcp/{}/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp", p2p_port));
		}

		let proc = cmd
			.stdout(process::Stdio::null())
			.stderr(process::Stdio::null())
			.stdin(process::Stdio::null())
			.spawn()
			.map_err(|e| {
				format!("Error spawning dkg node '{}': {}", BIN_PATH.to_string_lossy(), e)
			})?;

		processes.push(proc);
	}

	let ws_url = format!("ws://127.0.0.1:{}", port);

	// wait for rpc to be initialized
	const MAX_ATTEMPTS: u32 = 16;
	let mut attempts = 1;
	let mut wait_secs = 1;
	let client = loop {
		thread::sleep(Duration::from_secs(wait_secs));

		let result = ClientBuilder::new().set_url(ws_url.clone()).build().await;
		match result {
			Ok(client) => break Ok(client),
			Err(err) => {
				if attempts < MAX_ATTEMPTS {
					attempts += 1;
					wait_secs *= 2; // backoff
					continue
				}
				break Err(err)
			},
		}
	};

	match client {
		Ok(client) => Ok((client, processes, ws_url.clone())),
		Err(err) => {
			let err = format!(
				"Failed to connect to node rpc at {} after {} attempts: {}",
				ws_url, attempts, err
			);
			for mut proc in processes {
				let _ = proc
					.kill()
					.map_err(|e| format!("Error killing dkg process '{}': {}", proc.id(), e));
			}
			Err(err)
		},
	}
}

/// The start of the port range to scan.
const START_PORT: u16 = 9900;
/// The end of the port range to scan.
const END_PORT: u16 = 10000;
/// The maximum number of ports to scan before giving up.
const MAX_PORTS: u16 = 1000;
/// Next available unclaimed port for test node endpoints.
static PORT: AtomicU16 = AtomicU16::new(START_PORT);

/// Returns the next set of 3 open ports.
///
/// Returns None if there are not 3 open ports available.
fn next_open_port() -> Option<(u16, u16, u16)> {
	let mut ports = Vec::new();
	let mut ports_scanned = 0u16;
	loop {
		let _ = PORT.compare_exchange(END_PORT, START_PORT, Ordering::SeqCst, Ordering::SeqCst);
		let next = PORT.fetch_add(1, Ordering::SeqCst);
		if TcpListener::bind(("0.0.0.0", next)).is_ok() {
			ports.push(next);
			if ports.len() == 3 {
				return Some((ports[0], ports[1], ports[2]))
			}
		}
		ports_scanned += 1;
		if ports_scanned == MAX_PORTS {
			return None
		}
	}
}
