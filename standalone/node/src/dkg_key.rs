use std::path::PathBuf;

use sc_cli::{Error, SubstrateCli};
use sp_core::crypto::Pair;

use dkg_primitives::utils::{decrypt_data, StoredLocalKey};

/// Key utilities for the cli.
#[derive(Debug, clap::Subcommand)]
pub enum DKGKeySubcommand {
	/// Prints all the information sotred int the DKG LocalKey.
	Inspect(InspectKeyCmd),
}

impl DKGKeySubcommand {
	/// run the key subcommands
	pub fn run<C: SubstrateCli>(&self, _cli: &C) -> Result<(), Error> {
		match self {
			DKGKeySubcommand::Inspect(cmd) => cmd.run(),
		}
	}
}

/// Prints all the information sotred int the DKG LocalKey.
#[derive(Debug, clap::Parser)]
#[clap(
	name = "inspect-dkg-key",
	about = "Load an encrypted DKG key from a file and the secret used to encrypt it from the secret-file"
)]
pub struct InspectKeyCmd {
	/// The path to the local key file.
	#[clap(long)]
	pub dkg_file: PathBuf,
	/// The path to the secret file.
	#[clap(long)]
	pub secret_file: PathBuf,
}

impl InspectKeyCmd {
	/// run the key subcommands
	pub fn run(&self) -> Result<(), Error> {
		// Read the secret file
		let file_data = std::fs::read_to_string(&self.secret_file)
			.map_err(|e| Error::Input(format!("Failed to read secret file: {}", e)))?;
		// remove the qoutes and newlines
		let secret = file_data.replace('"', "").replace('\r', "").replace('\n', "");
		let (pair, _) = sp_core::sr25519::Pair::from_string_with_seed(&secret, None).map_err(|e| {
			Error::Input(format!("Failed to parse secret file: {:?}", e))
		})?;
		let secret = pair.to_raw_vec();
		// Read the encrypted local key file
		let encrypted_local_key = std::fs::read(&self.dkg_file)
			.map_err(|e| Error::Input(format!("Failed to read local key file: {}", e)))?;
		// Decrypt the local key
		let local_key = decrypt_data(encrypted_local_key, secret)?;
		// Deserialize the local key
		let local_key: StoredLocalKey = serde_json::from_slice(&local_key)
			.map_err(|e| Error::Input(format!("Failed to deserialize local key: {}", e)))?;
		// Pretty print the local key in json format
		let v = serde_json::to_string_pretty(&local_key)
			.map_err(|e| Error::Input(format!("Failed to serialize local key: {}", e)))?;
		println!("{v}");
		Ok(())
	}
}
