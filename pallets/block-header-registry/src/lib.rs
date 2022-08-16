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

//! # Block Header Registry Module
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

use frame_support::pallet_prelude::{ensure, DispatchError};
use sp_runtime::traits::{AtLeast32Bit, One, Zero};
use webb_primitives::webb_proposals::{
	evm::AnchorUpdateProposal, OnSignedProposal, Proposal, ProposalKind, ResourceId,
};

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;
	use webb_primitives::webb_proposals::ResourceId;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T, I = ()>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config<I: 'static = ()>: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self, I>> + IsType<<Self as frame_system::Config>::Event>;

		/// The origin which may forcibly reset parameters or otherwise alter
		/// privileged attributes.
		type ForceOrigin: EnsureOrigin<Self::Origin>;

		/// Weight information for the extrinsics
		type WeightInfo: WeightInfo;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
		pub phantom: (PhantomData<T>, PhantomData<I>),
	}

	#[cfg(feature = "std")]
	impl<T: Config<I>, I: 'static> Default for GenesisConfig<T, I> {
		fn default() -> Self {
			Self { phantom: Default::default() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config<I>, I: 'static> GenesisBuild<T, I> for GenesisConfig<T, I> {
		fn build(&self) {
		}
	}

	#[pallet::storage]
	#[pallet::getter(fn bridges)]
	/// Details of the module's parameters
	pub(super) type Bridges<T: Config<I>, I: 'static = ()> = StorageMap<
		_,
		Blake2_256,
		T::BridgeIndex,
		BridgeMetadata<T::MaxResources, T::MaxAdditionalFields>,
	>;

	#[pallet::event]
	pub enum Event<T: Config<I>, I: 'static = ()> {}

	#[pallet::error]
	pub enum Error<T, I = ()> {}

	#[pallet::hooks]
	impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {
		#[pallet::weight(0)]
		pub fn force_submit_unsigned_proposal(
			origin: OriginFor<T>,
			header: HeaderFor<T>,
			proof: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			Self::on_block_header(header, proof, true)
		}
	}
}

impl<T: Config> OnBlockHeader<DispatchError> Pallet<T> {
	on_block_header(header: &Header, proof: Proof, check_proof: bool) -> Result<(), DispatchError> {
		if check_proof {
			// check proof against proper chain method
		} else {
			// add header without proof (assumes through trusted governance method)
		}
	}
}