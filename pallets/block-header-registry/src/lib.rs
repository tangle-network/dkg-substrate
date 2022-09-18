// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
pub mod mock;
#[cfg(test)]
mod tests;

mod types;

use types::*;

use sp_std::{convert::TryInto, prelude::*};

use frame_support::pallet_prelude::{ensure, DispatchError};
use sp_runtime::traits::{AtLeast32Bit, One, Zero};
use webb_proposals::{
	evm::AnchorUpdateProposal, OnSignedProposal, Proposal, ProposalKind, ResourceId,
};

use webb_bridge_proofs::{
	traits::{BlockHeaderTrait, TypedChainIdTrait},
	validate_block_header
};

pub use pallet::*;

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

		/// Block header type
		type BlockHeader: BlockHeaderTrait;

		/// Typed chain Id type
		type TypedChainId: TypedChainIdTrait;
	}


	// Storage -------------------------------
	#[pallet::storage]
	#[pallet::getter(fn block_headers_by_chain)]
	pub type BlockHeadersByChain<T: Config> = StorageDoubleMap<
		_,
		Blake2_256,
		T::TypedChainId,
		Blake2_256,
		H256,
		T::BlockHeader
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn block_storage_limit_by_chain)]
	pub type BlockHeaderStorageLimitByChain<T: Config> = StorageMap<
		_,
		Blake2_256,
		T::TypedChainId,
		u32,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn current_storage_index_by_chain)]
	pub type CurrentStorageIndexByChain<T: Config> = StorageMap<
		_,
		Blake2_256,
		T::TypedChainId,
		u32,
		ValueQuery,
	>;
	// End Storage ----------------------------

	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event<T: Config> {
		//TODO: Add Events
	}

	#[pallet::error]
	pub enum Error<T> {
		/// TODO: Add Errors
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		pub fn add_block_header(
			origin: OriginFor<T>,
			header: T::BlockHeader,
		) -> DispatchResultWithPostInfo {
			if Self::validate_block_header(&header) {
					let id = header.typed_chain_id();
					let index = CurrentStorageIndexByChain::<T>::get(id).unwrap_or_default();
					let next_index = (index + 1) % BlockHeaderStorageLimitByChain::<T>::get(id).unwrap_or_default();
					BlockHeaders::<T>::insert(id, next_index, Some(header));
					CurrentStorageIndexByChain::<T>::insert(id, next_index);
			}
		}
	}	
}

impl<T: Config> Pallet<T> {
	// *** Helper/Utility Methods
	fn validate_block_header(
		header: &T::BlockHeader
	) -> Result<bool, DispatchError> {
		webb_bridge_proofs::validate_block_header(header)
	}
}

