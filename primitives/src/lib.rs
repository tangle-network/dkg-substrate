#![cfg_attr(not(feature = "std"), no_std)]

pub mod proposal;
pub use ethereum::*;
pub use ethereum_types::*;
pub use proposal::*;

use tiny_keccak::{Hasher, Keccak};

pub fn keccak_256(data: &[u8]) -> [u8; 32] {
	let mut keccak = Keccak::v256();
	keccak.update(data);
	let mut output = [0u8; 32];
	keccak.finalize(&mut output);
	output
}
