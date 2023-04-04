//! The test orchestrator accomplishes two things:
//! 1) Starts up the orchestrator/mock-blockchain server, and;
//! 2) Runs all the DKG nodes in child processes
//!
//! In summary, running this test orchestrator is an "all in one" replacement
//! for needing to run multiple clients. Each individual DKG node's stdout will be
//! piped to the temporary directory

use dkg_mock_blockchain::*;
use futures::TryStreamExt;
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
	name = "dkg-test-orchestrator",
	about = "Executes both the mock blockchain and client DKGs"
)]
struct Args {
	#[structopt(short = "c", long = "config")]
	// path to the configuration for the mock blockchain
	config_path: String,
	#[structopt(short = "t", long = "tmp")]
	tmp_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::from_args();
	log::info!(target: "dkg", "Orchestrator args: {args:?}");
	validate_args(&args)?;

	//let output = std::fs::File::create(args.tmp_path.join("output.json"))?;
	// before launching the DKGs, make sure to run to setup the logging
	dkg_logging::setup_simple_log();

	let data = tokio::fs::read_to_string(&args.config_path).await?;
	let config: MockBlockchainConfig = toml::from_str(&data)?;
	let n_clients = config.n_clients;
	let t = config.threshold;
	// set the number of blocks to the sum of the number of positive and negative cases
	// in other words, the each block gets 1 test case
	let n_blocks =
		config.positive_cases + config.error_cases.as_ref().map(|r| r.len()).unwrap_or(0);
	let bind_addr = config.bind.clone();

	// first, spawn the orchestrator/mock-blockchain
	let orchestrator_task = MockBlockchain::new(config).await?.execute();
	let orchestrator_handle = tokio::task::spawn(orchestrator_task);
	// give time for the orchestrator to bind
	tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

	let children_processes_dkg_clients = futures::stream::FuturesUnordered::new();
	// the gossip engine and the dummy api share a state between ALL clients in this process
	// we will use the SAME gossip engine for both keygen and signing
	let gossip_engine = &dkg_gadget::testing::InMemoryGossipEngine::new();
	let keygen_t = t as u16;
	let keygen_n = n_clients as u16;
	let signing_t = t as u16;
	let signing_n = n_clients as u16;

	// logging for the dummy api only
	let output = std::fs::File::create(args.tmp_path.join(format!("dummy_api.txt")))?;
	let dummy_api_logger =
		dkg_gadget::debug_logger::DebugLogger::new("dummy-api".to_string(), Some(output));

	let api = &dkg_gadget::testing::DummyApi::new(
		keygen_t,
		keygen_n,
		signing_t,
		signing_n,
		n_blocks,
		dummy_api_logger.clone(),
	);

	// setup the clients
	for idx in 0..n_clients {
		let latest_header = Arc::new(RwLock::new(None));
		let latest_test_uuid = Arc::new(RwLock::new(None));
		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		// pass the dummy api logger initially, with the intent of overwriting it later
		let mut key_store: dkg_gadget::keystore::DKGKeystore =
			dkg_gadget::keystore::DKGKeystore::new_default(dummy_api_logger.clone());
		let keyring = dkg_gadget::keyring::Keyring::Custom(idx as _);
		let mut keygen_gossip_engine = gossip_engine.clone_for_new_peer(
			api,
			n_blocks as _,
			keyring,
			key_store.as_dyn_crypto_store().unwrap(),
		);
		let mut signing_gossip_engine = keygen_gossip_engine.clone();

		// set the loggers for the gossip engines
		let (peer_id, _public_key) = keygen_gossip_engine.peer_id();
		let peer_id = *peer_id;
		// output the logs for this specific peer to a file
		let output = std::fs::File::create(args.tmp_path.join(format!("{peer_id}.txt")))?;
		let logger = dkg_gadget::debug_logger::DebugLogger::new(peer_id, Some(output));
		keygen_gossip_engine.set_logger(logger.clone());
		signing_gossip_engine.set_logger(logger.clone());
		key_store.set_logger(logger.clone());

		let client = Arc::new(
			dkg_gadget::testing::TestBackend::connect(
				&bind_addr,
				peer_id,
				api.clone(),
				rx,
				latest_test_uuid.clone(),
				logger.clone(),
			)
			.await?,
		);
		let backend = client.clone();
		let db_backend = Arc::new(dkg_gadget::db::DKGInMemoryDb::new());
		let metrics = None;
		let local_keystore = None;

		let child = async move {
			let label = peer_id.to_string();
			dkg_logging::define_span!("DKG Client", label);
			let dkg_worker_params = dkg_gadget::worker::WorkerParams {
				network: None,
				latest_header,
				client,
				backend,
				key_store,
				keygen_gossip_engine,
				signing_gossip_engine,
				db_backend,
				metrics,
				local_keystore,
				_marker: Default::default(),
			};

			let worker = dkg_gadget::worker::DKGWorker::new(
				dkg_worker_params,
				Some(tx),
				latest_test_uuid,
				logger,
			);
			worker.run().await;
			Err::<(), _>(std::io::Error::new(
				std::io::ErrorKind::Other,
				format!("Worker for peer {:?} ended", peer_id),
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
	let config_path = PathBuf::from(&args.config_path);
	let tmp_path = PathBuf::from(&args.tmp_path);
	if !config_path.is_file() {
		return Err(format!("{} is not a valid config path", args.config_path))
	}

	if !tmp_path.is_dir() {
		return Err(format!("{} is not a valid config path", args.config_path))
	}

	Ok(())
}
