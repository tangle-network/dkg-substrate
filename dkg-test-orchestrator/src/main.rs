//! The test orchestrator accomplishes two things:
//! 1) Starts up the orchestrator/mock-blockchain server, and;
//! 2) Runs all the DKG nodes in child processes
//!
//! In summary, running this test orchestrator is an "all in one" replacement
//! for needing to run multiple clients. Each individual DKG node's stdout will be
//! piped to the temporary directory
#![allow(clippy::unwrap_used)]
extern crate core;

// allow unwraps in tests
use crate::in_memory_gossip_engine::InMemoryGossipEngine;
use dkg_gadget::worker::TestBundle;
use dkg_mock_blockchain::*;
use dkg_runtime_primitives::{crypto, KEY_TYPE};
use futures::TryStreamExt;
use parking_lot::RwLock;
use sp_keystore::Keystore;
use std::{path::PathBuf, sync::Arc};
use structopt::StructOpt;

mod client;
mod dummy_api;
mod in_memory_gossip_engine;

#[derive(Debug, StructOpt)]
#[structopt(
	name = "dkg-test-orchestrator",
	about = "Executes both the mock blockchain and client DKGs"
)]
struct Args {
	#[structopt(short = "c", long = "config")]
	// path to the configuration for the mock blockchain
	config_path: Option<PathBuf>,
	#[structopt(short = "t", long = "tmp")]
	tmp_path: PathBuf,
	#[structopt(long)]
	clean: bool,
	#[structopt(long)]
	threshold: Option<u16>,
	#[structopt(long)]
	n: Option<u16>,
	#[structopt(long)]
	bind: Option<String>,
	#[structopt(long)]
	n_tests: Option<usize>,
	#[structopt(short = "p")]
	proposals_per_test: Option<usize>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::from_args();
	log::info!(target: "dkg", "Orchestrator args: {args:?}");
	validate_args(&args)?;

	//let output = std::fs::File::create(args.tmp_path.join("output.json"))?;
	// before launching the DKGs, make sure to run to setup the logging
	dkg_logging::setup_simple_log();

	let config = args_to_config(&args)?;
	let n_clients = config.n_clients;
	let t = config.threshold;
	// set the number of blocks to the sum of the number of positive and negative cases
	// in other words, the each block gets 1 test case
	let mut n_blocks =
		config.positive_cases + config.error_cases.as_ref().map(|r| r.len()).unwrap_or(0);
	n_blocks *= 2; // 2 test rounds per test case
	let bind_addr = config.bind.clone();

	// the gossip engine and the dummy api share a state between ALL clients in this process
	// we will use the SAME gossip engine for both keygen and signing
	let gossip_engine = &InMemoryGossipEngine::new();
	let keygen_t = t as u16;
	let keygen_n = n_clients as u16;
	let signing_t = t as u16;
	let signing_n = n_clients as u16;
	let blocks_per_session = 2; // do NOT change this value (for now)

	// logging for the dummy api only
	let output = args.tmp_path.join("dummy_api.log");
	let dummy_api_logger =
		dkg_gadget::debug_logger::DebugLogger::new("dummy-api".to_string(), Some(output))?;

	let api = &crate::dummy_api::DummyApi::new(
		keygen_t,
		keygen_n,
		signing_t,
		signing_n,
		dummy_api_logger.clone(),
		blocks_per_session,
	);

	let max_signing_sets_per_proposal =
		dkg_gadget::constants::signing_manager::MAX_POTENTIAL_RETRIES_PER_UNSIGNED_PROPOSAL;

	// first, spawn the orchestrator/mock-blockchain
	let orchestrator_task = MockBlockchain::new(
		config,
		api.clone(),
		dummy_api_logger.clone(),
		blocks_per_session,
		max_signing_sets_per_proposal as usize,
	)
	.await?
	.execute();
	let orchestrator_handle = tokio::task::spawn(orchestrator_task);
	// give time for the orchestrator to bind
	tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

	let children_processes_dkg_clients = futures::stream::FuturesUnordered::new();

	// setup the clients
	for idx in 0..n_clients {
		let latest_header = Arc::new(RwLock::new(None));
		let current_test_id = Arc::new(RwLock::new(None));
		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		// pass the dummy api logger initially, with the intent of overwriting it later
		let logger = dkg_gadget::debug_logger::DebugLogger::new("pre-init", None)?;
		let mut key_store: dkg_gadget::keystore::DKGKeystore =
			dkg_gadget::keystore::DKGKeystore::new_default(logger.clone());
		let keyring = dkg_gadget::keyring::Keyring::Custom(idx as _);

		let public_key: crypto::Public = Keystore::ecdsa_generate_new(
			key_store.as_dyn_crypto_store().unwrap(),
			KEY_TYPE,
			Some(&keyring.to_seed()),
		)
		.ok()
		.unwrap()
		.into();
		let peer_id = PeerId::random();
		// output the logs for this specific peer to a file
		let output = args.tmp_path.join(format!("{peer_id}.log"));
		let logger = dkg_gadget::debug_logger::DebugLogger::new(peer_id, Some(output))?;
		let gossip_engine = gossip_engine.clone_for_new_peer(
			api,
			n_blocks as _,
			peer_id,
			public_key.clone(),
			&logger,
		);

		key_store.set_logger(logger.clone());

		let client = Arc::new(
			crate::client::TestClient::connect(
				&bind_addr,
				peer_id,
				api.clone(),
				rx,
				current_test_id.clone(),
				logger.clone(),
			)
			.await?,
		);
		let backend = client.clone();
		let db_backend = Arc::new(dkg_gadget::db::DKGInMemoryDb::new());
		let metrics = None;
		let local_keystore = None;

		let child = async move {
			let _label = peer_id.to_string();
			dkg_logging::define_span!("DKG Client", _label);
			let test_bundle = TestBundle { to_test_client: tx, current_test_id };

			let dkg_worker_params = dkg_gadget::worker::WorkerParams {
				network: None,
				sync_service: None,
				latest_header,
				client,
				backend,
				key_store,
				gossip_engine,
				db_backend,
				metrics,
				local_keystore,
				test_bundle: Some(test_bundle),
				_marker: Default::default(),
			};

			let worker = dkg_gadget::worker::DKGWorker::new(dkg_worker_params, logger.clone());
			worker.run().await;
			logger.error("DKG Worker ended");
			Err::<(), _>(std::io::Error::new(
				std::io::ErrorKind::Other,
				format!("Worker for peer {peer_id:?} ended"),
			))
		};

		children_processes_dkg_clients.push(Box::pin(child));
	}

	tokio::select! {
		_res0 = orchestrator_handle => {
			Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Orchestrator ended prematurely")) as Box<dyn std::error::Error>)
		},
		res1 = children_processes_dkg_clients.try_collect::<Vec<()>>() => {
			res1.map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;
			Ok(())
		}
	}
}

fn validate_args(args: &Args) -> Result<(), String> {
	if let Some(config_path) = &args.config_path {
		if !config_path.is_file() {
			return Err(format!("{:?} is not a valid config path", args.config_path))
		}
	}

	let tmp_path = &args.tmp_path;

	if !tmp_path.is_dir() {
		return Err(format!("{:?} is not a valid tmp path", args.tmp_path))
	}

	if args.proposals_per_test.is_some() ||
		args.n.is_some() ||
		args.threshold.is_some() ||
		args.n_tests.is_some() ||
		args.bind.is_some()
	{
		if args.proposals_per_test.is_none() ||
			args.n.is_none() ||
			args.threshold.is_none() ||
			args.n_tests.is_none() ||
			args.bind.is_none()
		{
			return Err("If any of the following arguments are specified, all of them must be specified: proposals-per-test, n, threshold, n-tests, bind".to_string())
		}

		if args.config_path.is_some() {
			return Err("Either the config path or a manual set of args must be passed".to_string())
		}
	}

	if args.clean {
		std::fs::remove_dir_all(tmp_path)
			.map_err(|err| format!("Failed to clean tmp path: {err:?}"))?;
		std::fs::create_dir(tmp_path)
			.map_err(|err| format!("Failed to create tmp path: {err:?}"))?;
	}

	Ok(())
}

fn args_to_config(args: &Args) -> Result<MockBlockchainConfig, String> {
	if let Some(config_path) = &args.config_path {
		let config = std::fs::read_to_string(config_path)
			.map_err(|err| format!("Failed to read config file: {err:?}"))?;
		let config: MockBlockchainConfig = toml::from_str(&config)
			.map_err(|err| format!("Failed to parse config file: {err:?}"))?;
		Ok(config)
	} else {
		let n = args.n.unwrap() as _;
		let threshold = args.threshold.unwrap() as _;
		let n_tests = args.n_tests.unwrap();
		let proposals_per_test = args.proposals_per_test.unwrap();
		let bind = args.bind.clone().unwrap();
		let config = MockBlockchainConfig {
			threshold,
			min_simulated_latency: None,
			positive_cases: n_tests,
			error_cases: None,
			bind,
			n_clients: n,
			unsigned_proposals_per_session: Some(proposals_per_test),
		};
		Ok(config)
	}
}
