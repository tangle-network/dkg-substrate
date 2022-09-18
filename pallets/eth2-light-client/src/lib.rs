#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod tests;

mod types;

mod consensus;

use codec::{Codec, Decode, Encode};
use ethereum_types::{H256, U256};

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	// The pallet's runtime storage items.
	// https://docs.substrate.io/main-docs/build/runtime-storage/
	/// Whether the client validates the updates.
	/// Should only be set to `false` for debugging, testing, and diagnostic purposes
	#[pallet::storage]
	#[pallet::getter(fn validate_updates)]
	pub type validate_updates<T: Config> = StorageValue<_, bool, ValueQuery>;
	/// Whether the client verifies BLS signatures.
	#[pallet::storage]
	#[pallet::getter(fn verify_bls_signatures)]
	pub type verify_bls_signatures<T: Config> = StorageValue<_, bool, ValueQuery>;
	/// We store the hashes of the blocks for the past `hashes_gc_threshold` headers.
	/// Events that happen past this threshold cannot be verified by the client.
	/// It is desirable that this number is larger than 7 days' worth of headers, which is roughly
	/// 51k Ethereum blocks. So this number should be 51k in production.
	#[pallet::storage]
	#[pallet::getter(fn hashes_gc_threshold)]
	pub type hashes_gc_threshold = StorageValue<_, u64, ValueQuery>;
	/// Network. e.g. mainnet, kiln
	#[pallet::storage]
	#[pallet::getter(fn network)]
	pub type network: Network;
	/// Hashes of the finalized execution blocks mapped to their numbers. Stores up to
	/// `hashes_gc_threshold` entries. Execution block number -> execution block hash
	#[pallet::storage]
	#[pallet::getter(fn finalized_execution_blocks)]
	pub type finalized_execution_blocks = StorageMap<_, Blake2_128Concat, u64, H256, ValueQuery>;
	/// All unfinalized execution blocks' headers hashes mapped to their `HeaderInfo`.
	/// Execution block hash -> ExecutionHeaderInfo object
	#[pallet::storage]
	#[pallet::getter(fn unfinalized_headers)]
	pub type unfinalized_headers =
		StorageMap<_, Blake2_128Concat, H256, ExecutionHeaderInfo, ValueQuery>;
	/// `AccountId`s mapped to their number of submitted headers.
	/// Submitter account -> Num of submitted headers
	#[pallet::storage]
	#[pallet::getter(fn submitters)]
	pub type submitters = StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;
	/// Max number of unfinalized blocks allowed to be stored by one submitter account
	/// This value should be at least 32 blocks (1 epoch), but the recommended value is 1024 (32
	/// epochs)
	#[pallet::storage]
	#[pallet::getter(fn max_submitted_blocks_by_account)]
	pub type max_submitted_blocks_by_account = StorageValue<_, u32, ValueQuery>;
	// The minimum balance that should be attached to register a new submitter account
	#[pallet::storage]
	#[pallet::getter(fn min_storage_balance_for_submitter)]
	pub type min_storage_balance_for_submitter: Balance;
	/// Light client state
	#[pallet::storage]
	#[pallet::getter(fn finalized_beacon_header)]
	pub type finalized_beacon_header: ExtendedBeaconBlockHeader;
	#[pallet::storage]
	#[pallet::getter(fn finalized_execution_header)]
	pub type finalized_execution_header: LazyOption<ExecutionHeaderInfo>;
	#[pallet::storage]
	#[pallet::getter(fn current_sync_committee)]
	pub type current_sync_committee: LazyOption<SyncCommittee>;
	#[pallet::storage]
	#[pallet::getter(fn next_sync_committee)]
	pub type next_sync_committee: LazyOption<SyncCommittee>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/main-docs/build/events-errors/
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored(u32, T::AccountId),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// An example dispatchable that takes a singles value as a parameter, writes the value to
		/// storage and emits an event. This function must be dispatched by a signed extrinsic.
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn do_something(origin: OriginFor<T>, something: u32) -> DispatchResult {
			// Check that the extrinsic was signed and get the signer.
			// This function will return an error if the extrinsic is not signed.
			// https://docs.substrate.io/main-docs/build/origins/
			let who = ensure_signed(origin)?;

			// Update storage.
			<Something<T>>::put(something);

			// Emit an event.
			Self::deposit_event(Event::SomethingStored(something, who));
			// Return a successful DispatchResultWithPostInfo
			Ok(())
		}

		/// An example dispatchable that may throw a custom error.
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn cause_error(origin: OriginFor<T>) -> DispatchResult {
			let _who = ensure_signed(origin)?;

			// Read a value from storage.
			match <Something<T>>::get() {
				// Return an error if the value has not been set.
				None => return Err(Error::<T>::NoneValue.into()),
				Some(old) => {
					// Increment the value read from storage; will error in the event of overflow.
					let new = old.checked_add(1).ok_or(Error::<T>::StorageOverflow)?;
					// Update the value in storage with the incremented result.
					<Something<T>>::put(new);
					Ok(())
				},
			}
		}
	}
}
