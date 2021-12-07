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
use serde_json::{json, Value};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{Pair, Public, sr25519};
use sp_finality_grandpa::AuthorityId as GrandpaId;
use sp_runtime::traits::{IdentifyAccount, Verify};
use std::{
	process::{Child, ExitStatus},
	time::Duration,
};
use substrate_api_client::rpc::WsRpcClient;
use tokio::time::timeout;

static LOCALHOST_WS: &str = "ws://127.0.0.1:9945/";

pub fn json_req<S: Serialize>(method: &str, params: S, id: u32) -> Value {
	json!({
		"method": method,
		"params": params,
		"jsonrpc": "2.0",
		"id": id.to_string(),
	})
}

/// Generate a crypto pair from seed.
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

type AccountPublic = <Signature as Verify>::Signer;

/// Generate an account ID from seed.
pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId
where
	AccountPublic: From<<TPublic::Pair as Pair>::Public>,
{
	AccountPublic::from(get_from_seed::<TPublic>(seed)).into_account()
}

/// Generate the session keys from individual elements.
///
/// The input must be a tuple of individual keys (a single arg for now since we
/// have just one key).
pub fn dkg_session_keys(
	grandpa: GrandpaId,
	aura: AuraId,
	dkg: DKGId,
) -> dkg_standalone_runtime::opaque::SessionKeys {
	dkg_standalone_runtime::opaque::SessionKeys { grandpa, aura, dkg }
}

/// Wait for at least n blocks to be finalized within a specified time.
pub async fn wait_n_finalized_blocks(
	n: usize,
	timeout_secs: u64,
) -> Result<(), tokio::time::error::Elapsed> {
	timeout(Duration::from_secs(timeout_secs), wait_n_finalized_blocks_from(n, LOCALHOST_WS)).await
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

/// Extrinsics
use ac_compose_macros::compose_extrinsic;
use ac_primitives::{CallIndex, UncheckedExtrinsicV4};
use substrate_api_client::Pair as PairT;

pub type SetKeyFn = (CallIndex, dkg_standalone_runtime::opaque::SessionKeys, Vec<u8>);
pub type SetKeyXt = UncheckedExtrinsicV4<SetKeyFn>;

pub fn set_keys(
	api: &substrate_api_client::Api<
		sr25519::Pair,
		WsRpcClient,
	>,
	keys: dkg_standalone_runtime::opaque::SessionKeys,
	proof: Vec<u8>,
) -> SetKeyXt {
	compose_extrinsic!(api, "Session", "set_keys", keys, proof)
}
