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

// #[cfg(test)]
// pub mod mock;
// #[cfg(test)]
// mod tests;

mod evm_utils;
mod proof;
mod types;

use evm_utils::parse_evm_log;
use proof::evm::proof_traits::EVMProver;
use sp_runtime::DispatchError;
use types::*;

use sp_std::{convert::TryInto, prelude::*};

use dkg_runtime_primitives::{traits::BridgeRegistryTrait, ProposalHandlerTrait};
use frame_support::{pallet_prelude::ensure, traits::Get};
pub use pallet::*;
use webb_proposals::{
	evm::AnchorUpdateProposal, FunctionSignature, Nonce, ProposalHeader, ResourceId, TargetSystem,
	TypedChainId,
};

use eth_types::LogEntry;

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

		type BridgeRegistry: BridgeRegistryTrait;

		type ProposalHandler: ProposalHandlerTrait;

		#[pallet::constant]
		type AnchorUpdateFunctionSignature: Get<[u8; 4]>;

		type EVMProver: EVMProver;
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
		/// Invalid Typed Chain Id
		InvalidTypedChainId,
		/// Invalid EVM Log Entry Proof
		InvalidEvmLogEntryProof,
		/// No Bridge Found
		NoBridgeFound,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn submit_anchor_update_proposal(
			origin: OriginFor<T>,
			typed_chain_id: TypedChainId,
			proof_data: ProofData,
		) -> DispatchResultWithPostInfo {
			let _origin = ensure_signed(origin)?;
			match typed_chain_id {
				TypedChainId::Evm(_) => {
					let evm_proof_data = proof_data.to_evm_proof().unwrap().clone();
					ensure!(
						T::EVMProver::verify_log_entry(
							evm_proof_data.log_index,
							evm_proof_data.log_entry_data.clone(),
							evm_proof_data.receipt_index,
							evm_proof_data.receipt_data,
							evm_proof_data.header_data,
							evm_proof_data.proof,
						),
						Error::<T>::InvalidEvmLogEntryProof
					);
					// Submit the AnchorUpdateProposal
					Self::submit_anchor_update_proposals_evm(
						typed_chain_id,
						evm_proof_data.log_entry_data,
					)?;
					Ok(().into())
				},
				_ => Err(Error::<T>::InvalidTypedChainId.into()),
			}
		}
	}
}

impl<T: Config> Pallet<T> {
	pub fn submit_anchor_update_proposals_evm(
		typed_chain_id: TypedChainId,
		log_entry_data: Vec<u8>,
	) -> Result<(), DispatchError> {
		let (contract_address, merkle_root, nonce) = parse_evm_log(log_entry_data);
		let src_r_id =
			ResourceId::new(TargetSystem::ContractAddress(contract_address), typed_chain_id);
		let bridge_resource_ids = T::BridgeRegistry::get_bridge_for_resource(src_r_id);
		for r in bridge_resource_ids.unwrap_or_default() {
			if r == src_r_id {
				continue
			}
			let function_sig = FunctionSignature::from(T::AnchorUpdateFunctionSignature::get());
			let proposal_header = ProposalHeader::new(r, function_sig, Nonce::from(nonce));
			let proposal = AnchorUpdateProposal::new(proposal_header, merkle_root, src_r_id);
			T::ProposalHandler::handle_unsigned_proposal(
				proposal.to_bytes().to_vec(),
				dkg_runtime_primitives::ProposalAction::Sign(0),
			)?;
		}

		Ok(())
	}
}

impl<T: Config> EVMProver for Pallet<T> {}
