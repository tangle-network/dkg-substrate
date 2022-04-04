
//! Autogenerated weights for `pallet_dkg_metadata`
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 4.0.0-dev
//! DATE: 2022-04-02, STEPS: `50`, REPEAT: 20, LOW RANGE: `[]`, HIGH RANGE: `[]`
//! EXECUTION: Some(Wasm), WASM-EXECUTION: Compiled, CHAIN: Some("dev"), DB CACHE: 1024

// Executed Command:
// ./target/release/dkg-standalone-node
// benchmark
// --chain
// dev
// --execution
// wasm
// --wasm-execution
// compiled
// --pallet
// pallet-dkg-metadata
// --extrinsic
// *
// --steps
// 50
// --repeat
// 20
// --json
// --output
// ./pallets/dkg-metadata/src/weights.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]

use frame_support::{traits::Get, weights::Weight};
use sp_std::marker::PhantomData;

pub trait WeightInfo {
	fn set_signature_threshold() -> Weight;
	fn set_keygen_threshold() -> Weight;
	fn set_refresh_delay(_n:u32,) -> Weight;
	fn manual_increment_nonce() -> Weight;
	fn manual_refresh() -> Weight;
	fn set_time_to_restart() -> Weight;
	fn submit_public_key(n:u32,) -> Weight;
	fn submit_next_public_key(n:u32,) -> Weight;
	fn submit_misbehaviour_reports(n:u32,) -> Weight;
	fn submit_public_key_signature() -> Weight;
}

/// Weight functions for `pallet_dkg_metadata`.
pub struct WebbWeight<T>(PhantomData<T>);
impl<T: frame_system::Config>WeightInfo for WebbWeight<T> {
	// Storage: DKG NextAuthorities (r:1 w:0)
	// Storage: DKG PendingSignatureThreshold (r:1 w:1)
	fn set_signature_threshold() -> Weight {
		(13_000_000 as Weight)
			.saturating_add(T::DbWeight::get().reads(2 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG NextAuthorities (r:1 w:0)
	// Storage: DKG PendingSignatureThreshold (r:1 w:0)
	// Storage: DKG PendingKeygenThreshold (r:1 w:1)
	fn set_keygen_threshold() -> Weight {
		(15_000_000 as Weight)
			.saturating_add(T::DbWeight::get().reads(3 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG RefreshDelay (r:0 w:1)
	fn set_refresh_delay(_n: u32, ) -> Weight {
		(995_000 as Weight)
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG RefreshInProgress (r:1 w:0)
	// Storage: DKG RefreshNonce (r:1 w:1)
	fn manual_increment_nonce() -> Weight {
		(5_000_000 as Weight)
			.saturating_add(T::DbWeight::get().reads(2 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG TimeToRestart (r:0 w:1)
	fn set_time_to_restart() -> Weight {
		(1_000_000 as Weight)
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG RefreshInProgress (r:1 w:1)
	// Storage: DKG NextDKGPublicKey (r:1 w:0)
	// Storage: DKG RefreshNonce (r:1 w:1)
	// Storage: DKG ShouldManualRefresh (r:0 w:1)
	// Storage: DKGProposalHandler UnsignedProposalQueue (r:0 w:1)
	fn manual_refresh() -> Weight {
		(19_000_000 as Weight)
			.saturating_add(T::DbWeight::get().reads(3 as Weight))
			.saturating_add(T::DbWeight::get().writes(4 as Weight))
	}
	// Storage: DKG DKGPublicKey (r:1 w:1)
	// Storage: DKG CurrentAuthoritiesAccounts (r:1 w:0)
	// Storage: DKG NextSignatureThreshold (r:1 w:0)
	// Storage: DKG AuthoritySetId (r:1 w:0)
	fn submit_public_key(n: u32, ) -> Weight {
		(0 as Weight)
			// Standard Error: 15_556_000
			.saturating_add((1_997_122_000 as Weight).saturating_mul(n as Weight))
			.saturating_add(T::DbWeight::get().reads(4 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG NextDKGPublicKey (r:1 w:1)
	// Storage: DKG NextAuthoritiesAccounts (r:1 w:0)
	// Storage: DKG NextSignatureThreshold (r:1 w:0)
	// Storage: DKG AuthoritySetId (r:1 w:0)
	fn submit_next_public_key(n: u32, ) -> Weight {
		(0 as Weight)
			// Standard Error: 15_563_000
			.saturating_add((1_997_551_000 as Weight).saturating_mul(n as Weight))
			.saturating_add(T::DbWeight::get().reads(4 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG CurrentAuthoritiesAccounts (r:1 w:0)
	// Storage: DKG SignatureThreshold (r:1 w:0)
	// Storage: DKG AuthorityReputations (r:1 w:1)
	fn submit_misbehaviour_reports(n: u32, ) -> Weight {
		(0 as Weight)
			// Standard Error: 15_569_000
			.saturating_add((1_999_490_000 as Weight).saturating_mul(n as Weight))
			.saturating_add(T::DbWeight::get().reads(3 as Weight))
			.saturating_add(T::DbWeight::get().writes(1 as Weight))
	}
	// Storage: DKG NextDKGPublicKey (r:1 w:1)
	// Storage: DKG NextPublicKeySignature (r:1 w:1)
	// Storage: DKG UsedSignatures (r:1 w:1)
	// Storage: DKG RefreshNonce (r:1 w:0)
	// Storage: DKG DKGPublicKey (r:1 w:1)
	// Storage: DKG ShouldManualRefresh (r:1 w:1)
	// Storage: DKG AuthoritySetId (r:1 w:1)
	// Storage: DKG NextSignatureThreshold (r:1 w:1)
	// Storage: DKG NextKeygenThreshold (r:1 w:1)
	// Storage: DKG PendingSignatureThreshold (r:1 w:0)
	// Storage: DKG PendingKeygenThreshold (r:1 w:0)
	// Storage: DKG DKGPublicKeySignature (r:1 w:1)
	// Storage: System Digest (r:1 w:1)
	// Storage: DKG RefreshInProgress (r:0 w:1)
	// Storage: DKG KeygenThreshold (r:0 w:1)
	// Storage: DKG HistoricalRounds (r:0 w:1)
	// Storage: DKG PreviousPublicKey (r:0 w:1)
	// Storage: DKG SignatureThreshold (r:0 w:1)
	// Storage: DKGProposalHandler UnsignedProposalQueue (r:0 w:1)
	fn submit_public_key_signature() -> Weight {
		(218_000_000 as Weight)
			.saturating_add(T::DbWeight::get().reads(13 as Weight))
			.saturating_add(T::DbWeight::get().writes(16 as Weight))
	}
}

impl WeightInfo for () {

	fn set_signature_threshold() -> Weight { 0 }
	fn set_keygen_threshold() -> Weight { 0 }
	fn set_refresh_delay(_n: u32,) -> Weight { 0 }
	fn manual_increment_nonce() -> Weight { 0 }
	fn manual_refresh() -> Weight { 0 }
	fn set_time_to_restart() -> Weight { 0 }
	fn submit_public_key(_n:u32,) -> Weight { 0 }
	fn submit_next_public_key(_n:u32,) -> Weight { 0 }
	fn submit_misbehaviour_reports(_n:u32,) -> Weight { 0 }
	fn submit_public_key_signature() -> Weight { 0 }


}
