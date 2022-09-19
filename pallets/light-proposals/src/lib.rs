// This file is part of Webb.

// Copyright (C) 2021 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Bridge Registry Module
//!
//! A module for maintaining bridge metadata or views over connected
//! sets of anchors.
//!
//! ## Overview
//!
//! The Bridge Registry module provides functionality maintaing and storing
//! metadata about existing bridges.
//!
//! The supported dispatchable functions are documented in the [`Call`] enum.
//!
//! ### Terminology
//!
//! ### Goals
//!
//! ## Interface
//!
//! ## Related Modules
//!
//! * [`System`](../frame_system/index.html)
//! * [`Support`](../frame_support/index.html)

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
pub mod mock;
#[cfg(test)]
mod tests;

mod benchmarking;
mod types;

mod weights;
use weights::WeightInfo;

use types::*;

use sp_std::{convert::TryInto, prelude::*};

use frame_support::pallet_prelude::{ensure, DispatchError, DispatchResultWithPostInfo};
// pub use pallet::*;
use pallet_eth2_light_client::Eth2Prover;
use sp_runtime::traits::{AtLeast32Bit, One, Zero};
use webb_proposals::{
	evm::AnchorUpdateProposal, OnSignedProposal, Proposal, ProposalKind, ResourceId, TypedChainId,
};

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// Weight information for the extrinsics
		type WeightInfo: WeightInfo;

		type Eth2Prover: Eth2Prover;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub phantom: (PhantomData<T>),
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { phantom: Default::default() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {}
	}

	#[pallet::event]
	pub enum Event<T: Config> {}

	#[pallet::error]
	pub enum Error<T> {}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn verify_proof(
			origin: OriginFor<T>,
			typed_chain_id: TypedChainId,
			proof_data: ProofData,
		) -> DispatchResultWithPostInfo {
			match typed_chain_id {
				TypedChainId::Evm(_) => {
					let evm_proof_data = proof_data.to_evm_proof().unwrap();
					Ok(Eth2Prover::verify_log_entry(
						evm_proof_data.log_index,
						evm_proof_data.log_entry_data,
						evm_proof_data.receipt_index,
						evm_proof_data.receipt_data,
						evm_proof_data.header_data,
						evm_proof_data.proof,
					))
				},
				_ => (),
			}
		}
	}
}
