// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # DKG Proposal Handler Module
//!
//! A pallet to handle unsigned and signed proposals that are submitted for signing by the DKG.
//!
//! ## Overview
//!
//! The DKG Proposal Handler pallet is the pallet that directly handles the unsigned and
//! signed DKG proposals. It is responsible for maintaining the `UnsignedProposalQueue` that the
//! DKG authorities poll from for initiating threshold-signing. It is also responsible for the
//! submission of signed proposals back on-chain, which allows for external relayers to listen and
//! relay the signed proposals to their destinations.
//!
//! The pallet is meant to be used in conjunction with any governance system that processes
//! unsigned proposals either directly or indirectly such as the `pallet-dkg-proposals` pallet,
//! which delegates successfully voted upon proposals to the DKG Proposal Handler for processing.
//! This pallet also contains root-level functions that allow for the submission of unsigned
//! proposals that are useful for Webb Protocol applications. The intention being that tokenholders
//! of the Webb Protocol chain can vote through the `pallet-democracy` or a similar governance
//! system to submit unsigned proposals relevant for protocols built on the Webb Protocol's
//! interoperable private application platform.
//!
//! The signed proposals are submitted on-chain through an offchain worker and storage mechanism
//! that is maintained locally by each DKG authority. The DKG authorities engage in an offchain
//! multi-party ECDSA threshold signing protocol to sign the unsigned proposals. Once the DKG
//! authorities have signed proposals, they submit the signed proposals on-chain, where the
//! signatures are verified against the active DKG's public key.
//!
//! ### Terminology
//!
//! - Unsigned Proposal: A Proposal that is unsigned and is ready to be signed by the DKG
//!   authorities.
//! - Signed Proposal: A Proposals that is signed and contains a signature from the active DKG in
//!   the respective round.
//! - Unsigned Proposal Queue: A queue of unsigned proposals that are ready for signing.
//! - Anchor Update Proposal: A proposal for updating the merkle root state of an anchor on some
//!   compatible blockchain.
//! - Refresh Proposal: The proposal which rotates a soon-to-be outdated active DKG key to the
//!   soon-to-be active next DKG key.
//! - Proposer Set Update Proposal: The proposal which updates the latest proposer set from
//!   `pallet-dkg-proposals`.
//!
//! ### Implementation
//!
//! The DKG Proposal Handler pallet is implemented with the primary purpose of handling unsigned
//! proposals from the `pallet-dkg-proposals`, i.e. Anchor Update Proposals, handling forcefully
//! submitting unsigned proposals from the integrating chain's tokenholders, and handling the
//! submission of signed proposals back on-chain for data provenance and further processing.
//!
//! There are two main methods for submitting unsigned proposals currently implemented:
//! 1. `handle_unsigned_proposal` - A generic handler which expects raw Anchor Update Proposals.
//! 2. `force_submit_unsigned_proposal` - A root-level extrinsic that allows for the submission of
//! all other valid unsigned proposals
//!
//! Handled unsigned proposals are added to the `UnsignedProposalQueue` and are processed by the DKG
//! authorities offchain. The queue is polled using a runtime API and the multi-party ECDSA
//! threshold signing protocol is initiated for each proposal. Once the DKG authorities have signed
//! the unsigned proposal, the proposal is submitted on-chain and an RuntimeEvent is emitted.
//! Signed proposals are stored in the offchain storage system and polled each block by the offchain
//! worker system.
//!
//! The types of proposals available for submission is defined in the `ProposalType` enum as well as
//! the `DKGPayloadKey` enum. The list of currently supported proposals is as follows:
//! - Refresh: A proposal to refresh the DKG key across authority changes.
//! - ProposerSetUpdate: A proposal to update the proposer set from `pallet-dkg-proposals`.
//! - EVM: A generic EVM transaction proposal.
//! - AnchorCreate: A proposal to create an anchor on a compatible blockchain.
//! - AnchorUpdate: A proposal to update an anchor state on a compatible blockchain.
//! - TokenAdd: A proposal to add a token to system supporting a many-to-one token wrapper.
//! - TokenRemove: A proposal to remove a token from system supporting a many-to-one token wrapper.
//! - WrappingFeeUpdate: A proposal to update the wrapping fee for a many-to-one token wrapper.
//! - ResourceIdUpdate: A proposal to update or add a new resource ID to a system for registering
//!   resources.
//! - RescueTokens: A proposal to rescue tokens from a treasury based system.
//! - MaxDepositLimitUpdate: A proposal to update the maximum deposit limit for an escrow system.
//! - MinWithdrawalLimitUpdate: A proposal to update the minimal withdrawal limit for an escrow
//!   system.
//! - SetVerifier: A proposal to update the verifier for a zkSNARK based system.
//! - SetTreasuryHandler: A proposal to update the treasury handler for a treasury based system.
//! - FeeRecipientUpdate: A proposal to update the fee recipient for an escrow system.
//!
//! ### Rewards
//!
//! Currently, there are no extra rewards integrated for successfully signing proposals. This is a
//! future feature.
//!
//! ## Related Modules
//!
//! * [`System`](https://github.com/paritytech/substrate/tree/master/frame/system)
//! * [`Support`](https://github.com/paritytech/substrate/tree/master/frame/support)
//! * [`DKG Proposals`](../../pallet-dkg-proposals)

#![cfg_attr(not(feature = "std"), no_std)]

use dkg_runtime_primitives::{
	handlers::{decode_proposals::decode_proposal_identifier, validate_proposals::ValidationError},
	offchain::storage_keys::{OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK},
	DKGPayloadKey, OffchainSignedProposalBatches, ProposalAction, ProposalHandlerTrait,
	ProposalNonce, SignedProposalBatch, StoredUnsignedProposalBatch, TypedChainId,
};
use frame_support::pallet_prelude::*;
use frame_system::offchain::{AppCrypto, SendSignedTransaction, SignMessage, Signer};
pub use pallet::*;
use sp_runtime::{
	offchain::{
		storage::StorageValueRef,
		storage_lock::{StorageLock, Time},
	},
	traits::{AtLeast32BitUnsigned, Saturating, Zero},
};
use sp_std::{convert::TryInto, vec::Vec};
use webb_proposals::{OnSignedProposal, Proposal, ProposalKind};
pub use weights::WeightInfo;

mod impls;
pub use impls::*;

mod functions;
pub use functions::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;

#[frame_support::pallet]
pub mod pallet {
	use dkg_runtime_primitives::{utils::ensure_signed_by_dkg, DKGPayloadKey};
	use frame_support::dispatch::{fmt::Debug, DispatchError, DispatchResultWithPostInfo};
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use log;
	use sp_runtime::traits::{CheckedSub, One, Zero};
	use webb_proposals::{Proposal, ProposalKind};

	use super::*;

	pub type ProposalOf<T> = Proposal<<T as Config>::MaxProposalLength>;

	pub type StoredUnsignedProposalBatchOf<T> = StoredUnsignedProposalBatch<
		<T as Config>::BatchId,
		<T as Config>::MaxProposalLength,
		<T as Config>::MaxProposalsPerBatch,
		<T as frame_system::Config>::BlockNumber,
	>;

	pub type SignedProposalBatchOf<T> = SignedProposalBatch<
		<T as Config>::BatchId,
		<T as Config>::MaxProposalLength,
		<T as Config>::MaxProposalsPerBatch,
		<T as pallet_dkg_metadata::Config>::MaxSignatureLength,
	>;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + CreateSignedTransaction<Call<Self>> + pallet_dkg_metadata::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an
		/// RuntimeEvent.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// The identifier type for an offchain worker.
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;
		/// The signed proposal handler trait
		type SignedProposalHandler: OnSignedProposal<DispatchError, Self::MaxProposalLength>;
		/// Max number of signed proposal submissions per batch;
		#[pallet::constant]
		type MaxProposalsPerBatch: Get<u32>
			+ Debug
			+ Clone
			+ Eq
			+ PartialEq
			+ PartialOrd
			+ Ord
			+ TypeInfo;
		/// Max blocks to store an unsigned proposal
		#[pallet::constant]
		type UnsignedProposalExpiry: Get<Self::BlockNumber>;
		/// Max length of a proposal
		#[pallet::constant]
		type MaxProposalLength: Get<u32>
			+ Debug
			+ Clone
			+ Eq
			+ PartialEq
			+ PartialOrd
			+ Ord
			+ TypeInfo;
		// The batchId for a signed proposal batch
		type BatchId: Member
			+ Parameter
			+ Default
			+ Encode
			+ Decode
			+ AtLeast32BitUnsigned
			+ MaxEncodedLen
			+ Copy;

		/// The origin which may forcibly reset parameters or otherwise alter
		/// privileged attributes.
		type ForceOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Pallet weight information
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// All unsigned proposals.
	#[pallet::storage]
	#[pallet::getter(fn unsigned_proposal_queue)]
	pub type UnsignedProposalQueue<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		TypedChainId,
		Blake2_128Concat,
		T::BatchId,
		StoredUnsignedProposalBatchOf<T>,
	>;

	/// Defines the next batch id available
	#[pallet::storage]
	#[pallet::getter(fn next_batch_id)]
	pub(super) type NextBatchId<T: Config> = StorageValue<_, T::BatchId, ValueQuery>;

	/// Staging queue for unsigned proposals
	#[pallet::storage]
	#[pallet::getter(fn unsigned_proposals)]
	pub type UnsignedProposals<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		TypedChainId,
		BoundedVec<ProposalOf<T>, T::MaxProposalsPerBatch>,
	>;

	/// All signed proposals.
	#[pallet::storage]
	#[pallet::getter(fn signed_proposals)]
	pub type SignedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		TypedChainId,
		Blake2_128Concat,
		T::BatchId,
		SignedProposalBatchOf<T>,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// RuntimeEvent Emitted when we encounter a Proposal with invalid Signature.
		InvalidProposalBatchSignature {
			/// The list of proposals
			proposals: SignedProposalBatchOf<T>,
			/// Proposal Payload.
			data: Vec<u8>,
			/// The Invalid Signature.
			invalid_signature: Vec<u8>,
			/// Expected DKG Public Key (the one currently stored on chain).
			expected_public_key: Option<Vec<u8>>,
			/// The actual one we recovered from the data and signature.
			actual_public_key: Option<Vec<u8>>,
		},
		/// RuntimeEvent When a Proposal is added to UnsignedProposalQueue.
		ProposalAdded {
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Target Chain.
			target_chain: TypedChainId,
			/// The Proposal Data.
			data: Vec<u8>,
		},
		/// RuntimeEvent When a Proposal is removed from UnsignedProposalQueue.
		ProposalRemoved {
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Target Chain.
			target_chain: TypedChainId,
			/// Whether the proposal is due to expiration
			expired: bool,
		},
		/// RuntimeEvent When a Proposal Gets Signed by DKG.
		ProposalBatchSigned {
			/// The Target Chain.
			target_chain: TypedChainId,
			// The list of proposals signed
			proposals: SignedProposalBatchOf<T>,
			/// The Proposal Data.
			data: Vec<u8>,
			/// Signature of the hash of the proposal data.
			signature: Vec<u8>,
		},
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
		/// Proposal format is invalid
		ProposalFormatInvalid,
		/// Proposal must be unsigned
		ProposalMustBeUnsigned,
		/// Proposal bytes length is invalid
		InvalidProposalBytesLength,
		/// Proposal signature is invalid
		ProposalSignatureInvalid,
		/// No proposal with the ID was found
		ProposalDoesNotExists,
		/// Proposal with the ID has already been submitted
		ProposalAlreadyExists,
		/// Chain id is invalid
		ChainIdInvalid,
		/// Proposal length exceeds max allowed per batch
		ProposalsLengthOverflow,
		/// Proposal out of bounds
		ProposalOutOfBounds,
		/// Duplicate signed proposal
		CannotOverwriteSignedProposal,
		/// Unable to accept new unsigned proposal
		UnsignedProposalQueueOverflow,
		/// Math overflow
		ArithmeticOverflow,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			let res = Self::submit_signed_proposal_onchain(block_number);
			log::debug!(
				target: "runtime::dkg_proposal_handler",
				"offchain worker result: {:?}",
				res
			);
		}

		/// Hook that execute when there is leftover space in a block
		/// This function will execute on even blocks and move any proposals
		/// in unsigned proposals to unsigned proposal queue
		fn on_idle(now: T::BlockNumber, mut remaining_weight: Weight) -> Weight {
			use dkg_runtime_primitives::ProposalKind::*;

			// execute on even blocks
			if now % 2_u32.into() != 0_u32.into() {
				return remaining_weight
			}

			// fetch all unsigned proposals
			let unsigned_proposals: Vec<_> = UnsignedProposals::<T>::iter().collect();
			let unsigned_proposals_len = unsigned_proposals.len() as u64;
			remaining_weight =
				remaining_weight.saturating_sub(T::DbWeight::get().reads(unsigned_proposals_len));

			for (typed_chain_id, unsigned_proposals) in unsigned_proposals {
				remaining_weight =
					remaining_weight.saturating_sub(T::DbWeight::get().reads_writes(1, 3));

				if remaining_weight.is_zero() {
					break
				}

				// TODO : Handle failure gracefully
				let batch_id = Self::generate_next_batch_id().unwrap();
				// create new proposal batch
				let proposal_batch = StoredUnsignedProposalBatchOf::<T> {
					batch_id,
					proposals: unsigned_proposals,
					timestamp: <frame_system::Pallet<T>>::block_number(),
				};
				// push the batch to unsigned proposal queue
				UnsignedProposalQueue::<T>::insert(typed_chain_id, batch_id, proposal_batch);

				// remove the batch from the unsigned proposal list
				UnsignedProposals::<T>::remove(typed_chain_id);
			}

			remaining_weight
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		#[pallet::call_index(0)]
		#[frame_support::transactional]
		pub fn submit_signed_proposals(
			_origin: OriginFor<T>,
			props: Vec<SignedProposalBatchOf<T>>,
		) -> DispatchResultWithPostInfo {
			ensure!(
				props.len() <= T::MaxProposalsPerBatch::get() as usize,
				Error::<T>::ProposalsLengthOverflow
			);

			// log the caller, and the props.
			log::debug!(
				target: "runtime::dkg_proposal_handler",
				"submit_signed_proposal: props: {:?}",
				&props,
			);

			for prop_batch in &props {
				let data = prop_batch.data();

				// check the signature is valid
				let result = ensure_signed_by_dkg::<pallet_dkg_metadata::Pallet<T>>(
					&prop_batch.signature,
					&data,
				);
				match result {
					Ok(_) => {
						// Do nothing, it is all good.
					},
					Err(e) => {
						// this is a bad signature.
						// we emit it as an RuntimeEvent.
						Self::deposit_event(Event::InvalidProposalBatchSignature {
							proposals: prop_batch.clone(),
							data: data.clone().into(),
							expected_public_key: e.expected_public_key(),
							actual_public_key: e.actual_public_key(),
							invalid_signature: prop_batch.signature.clone().into(),
						});
						log::error!(
							target: "runtime::dkg_proposal_handler",
							"Invalid proposal signature with data: {:?}, sig: {:?} | ERR: {}",
							data,
							prop_batch.signature,
							e.ty()
						);
						// skip it.
						continue
					},
				}

				// now we need to log the data and signature
				log::debug!(
					target: "runtime::dkg_proposal_handler",
					"submit_signed_proposal: data: {:?}, signature: {:?}",
					data,
					prop_batch.signature
				);

				// lets mark each proposal as signed
				Self::handle_signed_proposal_batch(prop_batch.clone())?;

				continue
			}

			Ok(().into())
		}

		/// Force submit an unsigned proposal to the DKG
		///
		/// There are certain proposals we'd like to be proposable only
		/// through root actions. The currently supported proposals are
		/// 	1. Updating
		#[pallet::weight(1)]
		#[pallet::call_index(1)]
		pub fn force_submit_unsigned_proposal(
			origin: OriginFor<T>,
			prop: Proposal<T::MaxProposalLength>,
		) -> DispatchResultWithPostInfo {
			// Call must come from root (likely from a democracy proposal passing)
			<T as pallet::Config>::ForceOrigin::ensure_origin(origin)?;
			#[cfg(feature = "std")]
			println!("force_submit_unsigned_proposal: {:?}, {:?}", prop.data().len(), prop);
			// We ensure that only certain proposals are valid this way
			if prop.is_unsigned() {
				match decode_proposal_identifier(&prop) {
					Ok(v) => {
						Self::deposit_event(Event::<T>::ProposalAdded {
							key: v.key,
							target_chain: v.typed_chain_id,
							data: prop.data().clone(),
						});
						Self::store_unsigned_proposal(prop, v)?;
						Ok(().into())
					},
					Err(e) => Err(Self::handle_validation_error(e).into()),
				}
			} else {
				Err(Error::<T>::ProposalMustBeUnsigned.into())
			}
		}
	}

	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		/// Validate unsigned call to this module.
		///
		/// By default unsigned transactions are disallowed, but implementing the validator
		/// here we make sure that some particular calls (the ones produced by offchain worker)
		/// are being whitelisted and marked as valid.
		fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			// we allow calls only from the local OCW engine.
			match source {
				TransactionSource::Local | TransactionSource::InBlock => {},
				_ => return InvalidTransaction::Call.into(),
			}

			let current_block = <frame_system::Pallet<T>>::block_number();

			// Next, let's check that we call the right function.
			// Here we will use match stmt, to match over the call and see if it is
			// one of the functions we allow. if not we should return
			// `InvalidTransaction::Call.into()`.
			// we should handle the following calls:
			// 1. `submit_signed_proposals`
			// other than that we should return `InvalidTransaction::Call.into()`.
			let is_valid_call = matches! {
				call,
				Call::submit_signed_proposals { .. }
			};
			if !is_valid_call {
				frame_support::log::warn!(
					target: "runtime::dkg_metadata",
					"validate unsigned: invalid call: {:?}",
					call,
				);
				InvalidTransaction::Call.into()
			} else {
				frame_support::log::debug!(
					target: "runtime::dkg_metadata",
					"validate unsigned: valid call: {:?}",
					call,
				);
				ValidTransaction::with_tag_prefix("DKG")
					// We set base priority to 2**20 and hope it's included before any other
					// transactions in the pool. Next we tweak the priority by the current block,
					// so that transactions from older blocks are (more) included first.
					.priority(
						T::UnsignedPriority::get()
							.saturating_sub(current_block.try_into().unwrap_or_default()),
					)
					// The transaction is only valid for next 5 blocks. After that it's
					// going to be revalidated by the pool.
					.longevity(5)
					// It's fine to propagate that transaction to other peers, which means it can be
					// created even by nodes that don't produce blocks.
					// Note that sometimes it's better to keep it for yourself (if you are the block
					// producer), since for instance in some schemes others may copy your solution
					// and claim a reward.
					.propagate(true)
					.build()
			}
		}
	}
}
