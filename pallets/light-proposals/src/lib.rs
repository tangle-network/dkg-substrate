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

//! # Light Proposals Module
//!
//! A module for creating Webb Proposals from light-client proofs.
//!
//! ## Overview
//!
//! This module provides the ability to create a proposal from a light-client
//! proof. This enables a trustless and permissionless way to create proposals
//! for the Webb chain.
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
#![allow(dead_code)]
// #[cfg(test)]
// pub mod mock;
// #[cfg(test)]
// mod tests;

mod evm_utils;
mod proof;
mod traits;
mod types;

use sp_std::{convert::TryInto, prelude::*};

use dkg_runtime_primitives::{traits::BridgeRegistryTrait, ProposalHandlerTrait};
use frame_support::traits::Get;
pub use pallet::*;

use eth_types::LogEntry;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
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

		type BridgeRegistry: BridgeRegistryTrait;

		type ProposalHandler: ProposalHandlerTrait;

		#[pallet::constant]
		type AnchorUpdateFunctionSignature: Get<[u8; 4]>;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub phantom: PhantomData<T>,
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
	pub enum Error<T> {
		/// Invalid typed chain identifier
		InvalidTypedChainId,
		/// Invalid EVM log entry proof
		InvalidEvmLogEntryProof,
		/// No bridge found
		NoBridgeFound,
		/// Invalid proof data
		InvalidProofData,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}
}

impl<T: Config> proof::StorageVerifier<T> for Pallet<T> {}

impl<T: Config> traits::AnchorUpdateSubmitter<T> for Pallet<T> {
	type ProposalHandler = T::ProposalHandler;
	type BridgeRegistry = T::BridgeRegistry;
}

impl<T: Config> traits::AnchorUpdateCreator<T> for Pallet<T> {
	type AnchorUpdateSubmitter = Self;
	type StorageVerifier = Self;
}
