//! The test orchestrator accomplishes two things:
//! 1) Starts up the orchestrator/mock-blockchain server, and;
//! 2) Runs all the DKG nodes in child processes
//!
//! In summary, running this test orchestrator is an "all in one" replacement
//! for needing to run multiple clients. Each individual DKG node's stdout will be
//! piped to the temporary directory

use dkg_mock_blockchain::*;
use std::path::PathBuf;
use structopt::StructOpt;
use futures::TryStreamExt;

#[derive(Debug, StructOpt)]
#[structopt(
	name = "dkg-test-orchestrator",
	about = "Executes both the mock blockchain and client DKGs"
)]
struct Args {
	#[structopt(short = "t", long = "tmp")]
	// path to the temporary directory for piping stdout/stderr of each spawned DKG client child
	// process
	tmp: String,
	#[structopt(short = "c", long = "config")]
	// path to the configuration for the mock blockchain
	config_path: String,
	#[structopt(short = "d", long = "dkg")]
	// absolute path to the DKG executable (release build expected)
	dkg: String,
}

const NAMES: &'static [&'static str] = &["alice", "bob", "charlie", "dave", "eve", "ferdie"];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	dkg_logging::setup_log();
	let args = Args::from_args();
	log::info!(target: "dkg", "Orchestrator args: {args:?}");
	validate_args(&args)?;

	let data = tokio::fs::read_to_string(&args.config_path).await?;
	let config: MockBlockchainConfig = toml::from_str(&data)?;
	let n_client = config.n_clients;

	// first, spawn the orchestrator/mock-blockchain
	let orchestrator_task = MockBlockchain::new(config).await?.execute();
	let orchestrator_handle = tokio::task::spawn(orchestrator_task);

	// now, spawn the DKG clients
	let tmp_dir = PathBuf::from(args.tmp.clone());
	let children_processes_dkg_clients = futures::stream::FuturesUnordered::new();
	// setup the clients
	for idx in 0..n_client {
		let name_idx = idx % NAMES.len(); // cycle through each of the names
		let base_name = NAMES[name_idx];
		let unique_name = format!("{base_name}_{idx}");
		// create the output io:
		let mut tmp_file_stdout = tmp_dir.clone();
		tmp_file_stdout.push(format!("{unique_name}.stdout.txt"));
		log::info!("Will pipe stdout/stderr for {unique_name} to {}", tmp_file_stdout.display());

		let stdout_handle = std::fs::File::create(tmp_file_stdout)?;
		let stderr_handle = stdout_handle.try_clone()?;

		let mut cmd = tokio::process::Command::new(args.dkg.clone());
		cmd.arg("TestHarnessClient").arg("--tmp").arg("--{base_name}");
		// pipe both stdout and stderr to the tmp file
		cmd.stdout(stdout_handle);
		cmd.stderr(stderr_handle);

		let child = async move {
			match cmd.spawn()?.wait().await {
				Ok(exit_code) => {
					if exit_code.code().unwrap_or(-1) != 0 {
						Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Bad exit code: {exit_code}")))
					} else {
						Ok(())
					}
				},

				Err(err) => {
					Err(err)
				}
			}
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
	let tmp_dir = PathBuf::from(&args.tmp);
	if !tmp_dir.is_dir() {
		return Err(format!("{} is not a valid temporary directory", args.tmp))
	}

	let config_path = PathBuf::from(&args.config_path);
	if !config_path.is_file() {
		return Err(format!("{} is not a valid config path", args.config_path))
	}


	let dkg_path = PathBuf::from(&args.dkg);
	if !dkg_path.is_file() {
		return Err(format!("{} is not a valid DKG standalone binary path", args.dkg))
	}

	Ok(())
}