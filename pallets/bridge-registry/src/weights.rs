
//! Autogenerated weights for pallet_bridge_registry
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 4.0.0-dev
//! DATE: 2023-04-10, STEPS: `20`, REPEAT: `1`, LOW RANGE: `[]`, HIGH RANGE: `[]`
//! WORST CASE MAP SIZE: `1000000`
//! EXECUTION: Some(Wasm), WASM-EXECUTION: Compiled, CHAIN: Some("dev"), DB CACHE: 1024

// Executed Command:
// ./target/release/dkg-standalone-node
// benchmark
// pallet
// --chain=dev
// --steps=20
// --repeat=1
// --log=warn
// --pallet=pallet-bridge-registry
// --extrinsic=*
// --execution=wasm
// --wasm-execution=compiled
// --output=./pallets/bridge/src/weights.rs
// --template=./.maintain/frame-weight-template.hbs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use sp_std::marker::PhantomData;

/// Weight functions needed for pallet_bridge_registry.
pub trait WeightInfo {
	fn set_metadata() -> Weight;
	fn force_reset_indices() -> Weight;
}

/// Weights for pallet_bridge_registry using the Substrate node and recommended hardware.
pub struct WebbWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for WebbWeight<T> {
	// Storage: BridgeRegistry Bridges (r:1 w:1)
	fn set_metadata() -> Weight {
		Weight::from_ref_time(5_566_119)
			.saturating_add(Weight::from_proof_size(0))
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	// Storage: BridgeRegistry ResourceToBridgeIndex (r:0 w:1)
	fn force_reset_indices() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 13_000 nanoseconds.
		Weight::from_ref_time(13_000_000)
			.saturating_add(Weight::from_proof_size(0))
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
}

// For backwards compatibility and tests
impl WeightInfo for () {
		// Storage: BridgeRegistry Bridges (r:1 w:1)
	fn set_metadata() -> Weight {
		Weight::from_ref_time(5_566_119)
			.saturating_add(Weight::from_proof_size(0))
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	// Storage: BridgeRegistry ResourceToBridgeIndex (r:0 w:1)
	fn force_reset_indices() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 13_000 nanoseconds.
		Weight::from_ref_time(13_000_000)
			.saturating_add(Weight::from_proof_size(0))
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}
