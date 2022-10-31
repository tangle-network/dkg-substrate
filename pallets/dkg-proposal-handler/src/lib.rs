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
//! the unsigned proposal, the proposal is submitted on-chain and an event is emitted.
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
	handlers::decode_proposals::decode_proposal_identifier,
	offchain::storage_keys::{OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK},
	DKGPayloadKey, OffchainSignedProposals, ProposalAction, ProposalHandlerTrait, ProposalNonce,
	StoredUnsignedProposal, TypedChainId,
};
use frame_support::pallet_prelude::*;
use frame_system::offchain::{AppCrypto, SendSignedTransaction, Signer};
pub use pallet::*;
use sp_runtime::{
	offchain::{
		storage::StorageValueRef,
		storage_lock::{StorageLock, Time},
	},
	traits::Saturating,
};
use sp_std::{convert::TryInto, vec::Vec};
use webb_proposals::{OnSignedProposal, Proposal, ProposalKind};
pub use weights::WeightInfo;

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
	use frame_support::dispatch::{DispatchError, DispatchResultWithPostInfo};
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use log;
	use sp_runtime::traits::{CheckedSub, One, Zero};
	use webb_proposals::{Proposal, ProposalKind};

	use super::*;

	/// Unsigned proposal for this pallet
	pub type StoredUnsignedProposalOf<T> =
		StoredUnsignedProposal<<T as frame_system::Config>::BlockNumber>;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + CreateSignedTransaction<Call<Self>> + pallet_dkg_metadata::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// The identifier type for an offchain worker.
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;
		/// The signed proposal handler trait
		type SignedProposalHandler: OnSignedProposal<DispatchError>;
		/// Max number of signed proposal submissions per batch;
		#[pallet::constant]
		type MaxSubmissionsPerBatch: Get<u16>;
		/// Max blocks to store an unsigned proposal
		#[pallet::constant]
		type UnsignedProposalExpiry: Get<Self::BlockNumber>;
		/// Pallet weight information
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	/// All unsigned proposals.
	#[pallet::storage]
	#[pallet::getter(fn unsigned_proposals)]
	pub type UnsignedProposalQueue<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		TypedChainId,
		Blake2_128Concat,
		DKGPayloadKey,
		StoredUnsignedProposalOf<T>,
	>;

	/// Defines the block when next unsigned transaction will be accepted.
	///
	/// To prevent spam of unsigned (and unpayed!) transactions on the network,
	/// we only allow one transaction every `T::UnsignedInterval` blocks.
	/// This storage entry defines when new transaction is going to be accepted.
	#[pallet::storage]
	#[pallet::getter(fn next_unsigned_at)]
	pub(super) type NextUnsignedAt<T: Config> = StorageValue<_, T::BlockNumber, ValueQuery>;

	/// All signed proposals.
	#[pallet::storage]
	#[pallet::getter(fn signed_proposals)]
	pub type SignedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		TypedChainId,
		Blake2_128Concat,
		DKGPayloadKey,
		Proposal,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event Emitted when we encounter a Proposal with invalid Signature.
		InvalidProposalSignature {
			/// The Type of the Proposal.
			kind: ProposalKind,
			/// Proposal Payload.
			data: Vec<u8>,
			/// The Invalid Signature.
			invalid_signature: Vec<u8>,
			/// Expected DKG Public Key (the one currently stored on chain).
			expected_public_key: Option<Vec<u8>>,
			/// The actual one we recovered from the data and signature.
			actual_public_key: Option<Vec<u8>>,
		},
		/// Event When a Proposal is added to UnsignedProposalQueue.
		ProposalAdded {
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Target Chain.
			target_chain: TypedChainId,
			/// The Proposal Data.
			data: Vec<u8>,
		},
		/// Event When a Proposal is removed to UnsignedProposalQueue.
		ProposalRemoved {
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Target Chain.
			target_chain: TypedChainId,
		},
		/// Event When a Proposal Gets Signed by DKG.
		ProposalSigned {
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Target Chain.
			target_chain: TypedChainId,
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
		/// This function will look for any unsigned proposals past `UnsignedProposalExpiry`
		/// and remove storage.
		fn on_idle(now: T::BlockNumber, mut remaining_weight: Weight) -> Weight {
			// fetch all unsigned proposals
			let unsigned_proposals: Vec<_> = UnsignedProposalQueue::<T>::iter().collect();
			let unsigned_proposals_len = unsigned_proposals.len() as u64;
			remaining_weight =
				remaining_weight.saturating_sub(T::DbWeight::get().reads(unsigned_proposals_len));

			// filter out proposals to delete
			let unsigned_proposal_past_expiry = unsigned_proposals.into_iter().filter(
				|(_, _, StoredUnsignedProposal { timestamp, .. })| {
					let time_passed = now.checked_sub(timestamp).unwrap_or_default();
					time_passed > T::UnsignedProposalExpiry::get()
				},
			);

			// remove unsigned proposal until we run out of weight
			for expired_proposal in unsigned_proposal_past_expiry {
				remaining_weight =
					remaining_weight.saturating_sub(T::DbWeight::get().writes(One::one()));

				if remaining_weight.is_zero() {
					break
				}
				Self::deposit_event(Event::<T>::ProposalRemoved {
					target_chain: expired_proposal.0,
					key: expired_proposal.1,
				});
				UnsignedProposalQueue::<T>::remove(expired_proposal.0, expired_proposal.1);
			}

			remaining_weight
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(<T as Config>::WeightInfo::submit_signed_proposals(props.len() as u32))]
		#[frame_support::transactional]
		pub fn submit_signed_proposals(
			_origin: OriginFor<T>,
			props: Vec<Proposal>,
		) -> DispatchResultWithPostInfo {
			ensure!(
				props.len() <= T::MaxSubmissionsPerBatch::get() as usize,
				Error::<T>::ProposalsLengthOverflow
			);

			// log the caller, and the props.
			log::debug!(
				target: "runtime::dkg_proposal_handler",
				"submit_signed_proposal: props: {:?}",
				&props,
			);

			for prop in &props {
				if let Proposal::Signed { kind, data, signature } = prop {
					let result = ensure_signed_by_dkg::<pallet_dkg_metadata::Pallet<T>>(
						signature,
						&data[..],
					);
					match result {
						Ok(_) => {
							// Do nothing, it is all good.
						},
						Err(e) => {
							// this is a bad signature.
							// we emit it as an event.
							Self::deposit_event(Event::InvalidProposalSignature {
								kind: *kind,
								data: data.clone(),
								expected_public_key: e.expected_public_key(),
								actual_public_key: e.actual_public_key(),
								invalid_signature: signature.clone(),
							});
							log::error!(
								target: "runtime::dkg_proposal_handler",
								"Invalid proposal signature with kind: {:?}, data: {:?}, sig: {:?}",
								kind,
								data,
								signature
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
						signature
					);

					let prop = prop.clone();

					Self::handle_signed_proposal(prop)?;

					continue
				}

				return Err(Error::<T>::ProposalSignatureInvalid.into())
			}

			// now increment the block number at which we expect next unsigned transaction.
			let current_block = <frame_system::Pallet<T>>::block_number();
			<NextUnsignedAt<T>>::put(current_block + T::UnsignedInterval::get());
			Ok(().into())
		}

		/// Force submit an unsigned proposal to the DKG
		///
		/// There are certain proposals we'd like to be proposable only
		/// through root actions. The currently supported proposals are
		/// 	1. Updating
		#[pallet::weight(<T as Config>::WeightInfo::force_submit_unsigned_proposal())]
		pub fn force_submit_unsigned_proposal(
			origin: OriginFor<T>,
			prop: Proposal,
		) -> DispatchResultWithPostInfo {
			// Call must come from root (likely from a democracy proposal passing)
			ensure_root(origin)?;

			// We ensure that only certain proposals are valid this way
			if prop.is_unsigned() {
				match decode_proposal_identifier(&prop) {
					Ok(v) => {
						Self::deposit_event(Event::<T>::ProposalAdded {
							key: v.key,
							target_chain: v.typed_chain_id,
							data: prop.data().clone(),
						});
						UnsignedProposalQueue::<T>::insert(
							v.typed_chain_id,
							v.key,
							Self::stored_unsigned_proposal_from_unsigned_proposal(prop),
						);
						Ok(().into())
					},
					Err(_) => Err(Error::<T>::ProposalFormatInvalid.into()),
				}
			} else {
				Err(Error::<T>::ProposalFormatInvalid.into())
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
		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			// Now let's check if the transaction has any chance to succeed.
			let current_block = <frame_system::Pallet<T>>::block_number();
			let next_unsigned_at = <NextUnsignedAt<T>>::get();
			if next_unsigned_at > current_block {
				frame_support::log::debug!(
					target: "runtime::dkg_metadata",
					"validate unsigned: early block: current: {:?}, next_unsigned_at: {:?}",
					current_block,
					next_unsigned_at,
				);
				return InvalidTransaction::Stale.into()
			}
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
					// This transaction does not require anything else to go before into the pool.
					// In theory we could require `previous_unsigned_at` transaction to go first,
					// but it's not necessary in our case.
					//.and_requires()
					// We set the `provides` tag to be the same as `next_unsigned_at`. This makes
					// sure only one transaction produced after `next_unsigned_at` will ever
					// get to the transaction pool and will end up in the block.
					// We can still have multiple transactions compete for the same "spot",
					// and the one with higher priority will replace other one in the pool.
					.and_provides(next_unsigned_at)
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

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	fn handle_unsigned_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		let proposal = Proposal::Unsigned { data: proposal, kind: ProposalKind::AnchorUpdate };
		if let Ok(v) = decode_proposal_identifier(&proposal) {
			Self::deposit_event(Event::<T>::ProposalAdded {
				key: v.key,
				target_chain: v.typed_chain_id,
				data: proposal.data().clone(),
			});
			UnsignedProposalQueue::<T>::insert(
				v.typed_chain_id,
				v.key,
				Self::stored_unsigned_proposal_from_unsigned_proposal(proposal),
			);
			return Ok(())
		}

		Err(Error::<T>::ProposalFormatInvalid.into())
	}

	fn handle_unsigned_proposer_set_update_proposal(
		proposal: Vec<u8>,
		_action: ProposalAction,
	) -> DispatchResult {
		let unsigned_proposal =
			Proposal::Unsigned { data: proposal, kind: ProposalKind::ProposerSetUpdate };
		if let Ok(v) = decode_proposal_identifier(&unsigned_proposal) {
			Self::deposit_event(Event::<T>::ProposalAdded {
				key: v.key,
				target_chain: v.typed_chain_id,
				data: unsigned_proposal.data().clone(),
			});

			UnsignedProposalQueue::<T>::insert(
				v.typed_chain_id,
				v.key,
				Self::stored_unsigned_proposal_from_unsigned_proposal(unsigned_proposal),
			);

			return Ok(())
		}

		Err(Error::<T>::ProposalFormatInvalid.into())
	}

	fn handle_unsigned_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		let unsigned_proposal =
			Proposal::Unsigned { data: proposal.encode(), kind: ProposalKind::Refresh };

		Self::deposit_event(Event::<T>::ProposalAdded {
			key: DKGPayloadKey::RefreshVote(proposal.nonce),
			target_chain: TypedChainId::None,
			data: unsigned_proposal.data().clone(),
		});

		// Add new refresh proposal to the queue
		UnsignedProposalQueue::<T>::insert(
			TypedChainId::None,
			DKGPayloadKey::RefreshVote(proposal.nonce),
			Self::stored_unsigned_proposal_from_unsigned_proposal(unsigned_proposal),
		);

		Ok(())
	}

	fn handle_signed_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		// Attempt to remove all previous unsigned refresh proposals too
		// This may also remove ProposerSetUpdate proposals that haven't been signed
		// yet, but given that this action is only to clean storage when a refresh
		// fails, we can assume that the previous proposer set update will nonetheless
		// need to be used to update the governors on the respective webb Apps anyway.
		let remaining_untyped_proposals: usize =
			UnsignedProposalQueue::<T>::iter_key_prefix(TypedChainId::None).count();

		for i in 0..remaining_untyped_proposals {
			let index = i as u32;
			// Ensure we break when we reach the bottom
			if proposal.nonce.saturating_sub(ProposalNonce(index)) == ProposalNonce(0u32) {
				break
			}
			// Otherwise continue removing old refresh votes
			UnsignedProposalQueue::<T>::remove(
				TypedChainId::None,
				DKGPayloadKey::RefreshVote(proposal.nonce.saturating_sub(ProposalNonce(index))),
			);

			Self::deposit_event(Event::<T>::ProposalRemoved {
				key: DKGPayloadKey::RefreshVote(
					proposal.nonce.saturating_sub(ProposalNonce(index)),
				),
				target_chain: TypedChainId::None,
			});
		}

		Ok(())
	}

	fn handle_signed_proposal(prop: Proposal) -> DispatchResult {
		let id =
			decode_proposal_identifier(&prop).map_err(|_e| Error::<T>::ProposalFormatInvalid)?;
		// Log the chain id and nonce
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: chain: {:?}, payload_key: {:?}",
			id.typed_chain_id,
			id.key,
		);

		ensure!(
			UnsignedProposalQueue::<T>::contains_key(id.typed_chain_id, id.key),
			Error::<T>::ProposalDoesNotExists
		);
		// Log that proposal exist in the unsigned queue
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: proposal exist in the unsigned queue"
		);
		let (data, sig) = match prop.signature() {
			Some(sig) => (prop.data().clone(), sig),
			None => return Err(Error::<T>::ProposalSignatureInvalid.into()),
		};
		ensure!(
			Self::validate_proposal_signature(&data, &sig),
			Error::<T>::ProposalSignatureInvalid
		);
		// Log that the signature is valid
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: signature is valid"
		);
		// Update storage
		SignedProposals::<T>::insert(id.typed_chain_id, id.key, prop.clone());
		UnsignedProposalQueue::<T>::remove(id.typed_chain_id, id.key);
		// Emit event so frontend can react to it.
		Self::deposit_event(Event::<T>::ProposalSigned {
			key: id.key,
			target_chain: id.typed_chain_id,
			data: data.to_vec(),
			signature: sig.to_vec(),
		});
		// Finally let any handlers handle the signed proposal
		T::SignedProposalHandler::on_signed_proposal(prop)?;
		Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposals() -> Vec<dkg_runtime_primitives::UnsignedProposal> {
		UnsignedProposalQueue::<T>::iter()
			.map(|(typed_chain_id, key, stored_unsigned_proposal)| {
				dkg_runtime_primitives::UnsignedProposal {
					typed_chain_id,
					key,
					proposal: stored_unsigned_proposal.proposal,
				}
			})
			.collect()
	}

	/// Checks whether a signed proposal exists in the `SignedProposals` storage
	pub fn is_not_existing_proposal(prop: &Proposal) -> bool {
		if prop.is_signed() {
			match decode_proposal_identifier(prop) {
				Ok(v) => !SignedProposals::<T>::contains_key(v.typed_chain_id, v.key),
				Err(_) => false,
			}
		} else {
			false
		}
	}

	/// Returns `StoredUnsignedProposal` from proposal by inserting current BlockNumber
	pub fn stored_unsigned_proposal_from_unsigned_proposal(
		proposal: Proposal,
	) -> StoredUnsignedProposalOf<T> {
		let timestamp = <frame_system::Pallet<T>>::block_number();
		StoredUnsignedProposalOf::<T> { proposal, timestamp }
	}

	// *** Offchain worker methods ***

	/// Offchain worker function that submits signed proposals from the offchain storage on-chain
	///
	/// The function submits batches of signed proposals on-chain in batches of
	/// `T::MaxSubmissionsPerBatch`. Proposals are stored offchain and target specific block numbers
	/// for submission. This function polls all relevant proposals ready for submission at the
	/// current block number
	fn submit_signed_proposal_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let next_unsigned_at = <NextUnsignedAt<T>>::get();
		if next_unsigned_at > block_number {
			return Err("Too early to send unsigned transaction")
		}
		let mut lock = StorageLock::<Time>::new(SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK);
		{
			let _guard = lock.lock();

			let signer = Signer::<T, <T as Config>::OffChainAuthId>::all_accounts();
			if !signer.can_sign() {
				return Err(
					"No local accounts available. Consider adding one via `author_insertKey` RPC.",
				)
			}
			match Self::get_next_offchain_signed_proposal(block_number) {
				Ok(next_proposals) => {
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"submit_signed_proposal_onchain: found {} proposals to submit before filtering\n {:?}",
						next_proposals.len(), next_proposals
					);
					// We filter out all proposals that are already on chain
					let filtered_proposals = next_proposals
						.iter()
						.cloned()
						.filter(Self::is_not_existing_proposal)
						.collect::<Vec<_>>();
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"submit_signed_proposal_onchain: found {} proposals to submit after filtering\n {:?}",
						filtered_proposals.len(), filtered_proposals
					);
					// We split the vector into chunks of `T::MaxSubmissionsPerBatch` length and
					// submit those chunks
					for chunk in
						filtered_proposals.chunks(T::MaxSubmissionsPerBatch::get() as usize)
					{
						let call = Call::<T>::submit_signed_proposals { props: chunk.to_vec() };
						let result = signer
							.send_signed_transaction(|_| call.clone())
							.into_iter()
							.map(|(_, r)| r)
							.collect::<Result<Vec<_>, _>>()
							.map_err(|()| "Unable to submit unsigned transaction.");
						// Display error if the signed tx fails.
						if result.is_err() {
							log::error!(
								target: "runtime::dkg_proposal_handler",
								"failure: failed to send unsigned transaction to chain: {:?}",
								call,
							);
						} else {
							// log the result of the transaction submission
							log::debug!(
								target: "runtime::dkg_proposal_handler",
								"Submitted unsigned transaction for signed proposal: {:?}",
								call,
							);
						}
					}
				},
				Err(e) => {
					// log the error
					log::warn!(
						target: "runtime::dkg_proposal_handler",
						"Failed to get next signed proposal: {}",
						e
					);
				},
			};
			Ok(())
		}
	}

	/// Returns the list of signed proposals ready for on-chain submission at the given
	/// `block_number`
	fn get_next_offchain_signed_proposal(
		block_number: T::BlockNumber,
	) -> Result<Vec<Proposal>, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut all_proposals = Vec::new();
		let res = proposals_ref.mutate::<OffchainSignedProposals<T::BlockNumber>, _, _>(|res| {
			match res {
				Ok(Some(mut prop_wrapper)) => {
					// log the proposals
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"Offchain signed proposals: {:?}",
						prop_wrapper.proposals
					);
					// log how many proposal batches are left
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"Offchain signed proposals left: {}",
						prop_wrapper.proposals.len()
					);
					// We get all batches whose submission delay has been satisfied
					all_proposals = prop_wrapper
						.proposals
						.iter()
						.filter_map(
							|(props, submit_at)| {
								if *submit_at <= block_number {
									Some(props)
								} else {
									None
								}
							},
						)
						.flatten()
						.cloned()
						.collect::<Vec<_>>();
					// then we need to keep only the batches that are not yet submitted
					prop_wrapper.proposals.retain(|(_, submit_at)| *submit_at > block_number);
					Ok(prop_wrapper)
				},
				Ok(None) => Err("No signed proposals key stored"),
				Err(e) => {
					// log the error
					log::warn!(
						target: "runtime::dkg_proposal_handler",
						"Failed to read offchain signed proposals: {:?}",
						e
					);
					Err("Error decoding offchain signed proposals")
				},
			}
		});

		if res.is_err() {
			return Err("Unable to get next proposal batch")
		}

		Ok(all_proposals)
	}

	// *** Validation methods ***

	fn validate_proposal_signature(data: &[u8], signature: &[u8]) -> bool {
		dkg_runtime_primitives::utils::validate_ecdsa_signature(data, signature)
	}

	// *** Utility methods ***

	#[cfg(feature = "runtime-benchmarks")]
	pub fn signed_proposals_len() -> usize {
		SignedProposals::<T>::iter_keys().count()
	}
}
