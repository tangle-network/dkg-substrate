//! The test orchestrator accomplishes two things:
//! 1) Starts up the orchestrator/mock-blockchain server, and;
//! 2) Runs all the DKG nodes in child processes
//!
//! In summary, running this test orchestrator is an "all in one" replacement
//! for needing to run multiple clients. Each individual DKG node's stdout will be
//! piped to the temporary directory

use dkg_mock_blockchain::*;
use futures::TryStreamExt;
use std::path::PathBuf;
use structopt::StructOpt;
use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, StructOpt)]
#[structopt(
	name = "dkg-test-orchestrator",
	about = "Executes both the mock blockchain and client DKGs"
)]
struct Args {
	#[structopt(short = "c", long = "config")]
	// path to the configuration for the mock blockchain
	config_path: String,
}

//const NAMES: &'static [&'static str] = &["alice", "bob", "charlie", "dave", "eve", "ferdie"];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	dkg_logging::setup_log();
	let args = Args::from_args();
	log::info!(target: "dkg", "Orchestrator args: {args:?}");
	validate_args(&args)?;

	let data = tokio::fs::read_to_string(&args.config_path).await?;
	let config: MockBlockchainConfig = toml::from_str(&data)?;
	let n_client = config.n_clients;
	let bind_addr = config.bind.clone();

	// first, spawn the orchestrator/mock-blockchain
	let orchestrator_task = MockBlockchain::new(config).await?.execute();
	let orchestrator_handle = tokio::task::spawn(orchestrator_task);
	// give time for the orchestrator to bind
	tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

	let children_processes_dkg_clients = futures::stream::FuturesUnordered::new();
	let gossip_engine = &dkg_gadget::testing::InMemoryGossipEngine::new();
	// setup the clients
	for _idx in 0..n_client {
		//let name_idx = idx % NAMES.len(); // cycle through each of the names
		//let base_name = NAMES[name_idx];
		//let unique_name = format!("{base_name}_{idx}");
		
		let latest_header = Arc::new(RwLock::new(None));
		// using clone_for_new_peer then clone ensures the peer ID instances are the same
		let keygen_gossip_engine = gossip_engine.clone_for_new_peer();
		let signing_gossip_engine = keygen_gossip_engine.clone();
		let peer_id = keygen_gossip_engine.peer_id().clone();
		
		let client = Arc::new(dkg_gadget::testing::TestBackend::connect(&bind_addr, peer_id).await?);
		let backend = client.clone();
		let key_store: dkg_gadget::keystore::DKGKeystore = None.into();
		let db_backend = Arc::new(dkg_gadget::db::DKGInMemoryDb::new());
		let metrics = None;
		let local_keystore = None;

		let child = async move {
			let dkg_worker_params = dkg_gadget::worker::WorkerParams {
				latest_header,
				client,
				backend,
				key_store,
				keygen_gossip_engine,
				signing_gossip_engine,
				db_backend,
				metrics,
				local_keystore,
				_marker: Default::default()
			};

			let worker = dkg_gadget::worker::DKGWorker::new(dkg_worker_params);
			worker.run().await;
			Err::<(), _>(std::io::Error::new(std::io::ErrorKind::Other, format!("Worker for peer {:?} ended", peer_id)))
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
	if !config_path.is_file() {
		return Err(format!("{} is not a valid config path", args.config_path))
	}

	Ok(())
}
