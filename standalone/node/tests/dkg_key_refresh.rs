#![cfg(unix)]
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
	time::Duration,
};

use subxt::{ClientBuilder, PairSigner};

pub mod common;

use common::{get_pair_signer, spawn, Spawnable};
use webb::substrate::dkg_runtime::{
	self,
	api::runtime_types::{pallet_staking::ValidatorPrefs, sp_arithmetic::per_things::Perbill},
};

use std::path::PathBuf;

#[tokio::test]
async fn dkg_key_refresh() -> Result<(), subxt::Error> {
	env_logger::init();

	let (client, processes, ws_url, mut temp_dirs) =
		spawn(vec![Spawnable::Alice, Spawnable::Charlie, Spawnable::Bob])
			.await
			.map_err(|e| subxt::Error::Other(e))?;

	let alice = &processes[0];
	let charlie = &processes[1];
	let bob = &processes[2];

	let signer =
		PairSigner::<dkg_runtime::api::DefaultConfig, _>::new(AccountKeyring::Alice.pair());

	let charlie_stash = get_pair_signer("Charlie//stash");

	let api =
		client.to_runtime_api::<dkg_runtime::api::RuntimeApi<dkg_runtime::api::DefaultConfig>>();

	// Session 0

	let _hash = api.tx().staking().chill().sign_and_submit(&charlie_stash).await?;

	let _hash = api.tx().staking().force_new_era().sign_and_submit(&signer).await?;

	let _ = common::wait_n_finalized_blocks(45, &ws_url, 540).await;

	let next_key: Option<(u64, Vec<u8>)> = api.storage().dkg().next_dkg_public_key(None).await?;

	let next_public_key_signature: Option<(u64, Vec<u8>)> =
		api.storage().dkg().next_public_key_signature(None).await?;

	let dkg_public_key: (u64, Vec<u8>) = api.storage().dkg().dkg_public_key(None).await?;

	assert!(next_key.is_some(), "Next public key should be on chain");

	assert!(next_public_key_signature.is_some(), "Next public key signature should be on chain");

	assert!(!dkg_public_key.1.is_empty(), "DKG public key should be on chain");

	let _ = common::wait_n_finalized_blocks(6, &ws_url, 72).await;

	// Session 1

	let _hash = api
		.tx()
		.staking()
		.validate(ValidatorPrefs { commission: Perbill(0), blocked: false })
		.sign_and_submit(&charlie_stash)
		.await?;

	let _hash = api.tx().staking().force_new_era().sign_and_submit(&signer).await?;

	let _ = common::wait_n_finalized_blocks(15, &ws_url, 180).await;

	// New session has started but authority set has not changed since the authority set for genesis session
	// and second session is the same, queued dkg set should have changed so the next dkg public key should be different

	let next_key_1: Option<(u64, Vec<u8>)> = api.storage().dkg().next_dkg_public_key(None).await?;

	assert!(
		(next_key_1.is_some() && (next_key_1 != next_key)),
		"Next public key should be on chain"
	);

	let _ = common::wait_n_finalized_blocks(35, &ws_url, 540).await;

	let next_public_key_signature_1: Option<(u64, Vec<u8>)> =
		api.storage().dkg().next_public_key_signature(None).await?;

	assert!(
		(next_public_key_signature_1.is_some() &&
			(next_public_key_signature_1 != next_public_key_signature)),
		"Next public key should be on chain"
	);

	let _ = common::wait_n_finalized_blocks(10, &ws_url, 120).await;

	// Session 2

	// A key refresh should have occured

	let used_signatures: Vec<Vec<u8>> = api.storage().dkg().used_signatures(None).await?;

	assert!(
		used_signatures.len() == 1 && used_signatures[0] == next_public_key_signature_1.unwrap().1,
		"Used signatures should contain one value"
	);

	let _ = common::wait_n_finalized_blocks(50, &ws_url, 600).await;

	// Session 3

	// A key refresh should have occured

	let used_signatures: Vec<Vec<u8>> = api.storage().dkg().used_signatures(None).await?;

	assert!(used_signatures.len() == 2, "Used signatures should contain two values");

	// Stop the processes
	kill(Pid::from_raw(alice.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(bob.id().try_into().unwrap()), SIGINT).unwrap();
	kill(Pid::from_raw(charlie.id().try_into().unwrap()), SIGINT).unwrap();

	tokio::time::sleep(Duration::from_secs(1)).await;

	for tmp_dir in temp_dirs.iter_mut() {
		let _ = tmp_dir.take().unwrap().close();
	}
	Ok(())
}
