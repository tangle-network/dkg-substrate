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

use dkg_runtime_primitives::handlers::decode_proposals::decode_proposal;

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;
use dkg_runtime_primitives::{
	offchain::storage_keys::{OFFCHAIN_SIGNED_PROPOSALS, SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK},
	ChainIdTrait, ChainIdType, DKGPayloadKey, OffchainSignedProposals, Proposal, ProposalAction,
	ProposalHandlerTrait, ProposalKind,
};
use frame_support::pallet_prelude::*;
use frame_system::{
	offchain::{AppCrypto, SendSignedTransaction, Signer},
	pallet_prelude::OriginFor,
};
use sp_runtime::{
	offchain::{
		storage::StorageValueRef,
		storage_lock::{StorageLock, Time},
	},
	traits::Zero,
};
use sp_std::vec::Vec;

pub mod weights;
use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use dkg_runtime_primitives::{
		handlers::decode_proposals::decode_proposal, utils::ensure_signed_by_dkg, DKGPayloadKey,
		Proposal, ProposalKind,
	};
	use frame_support::dispatch::DispatchResultWithPostInfo;
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use sp_runtime::traits::AtLeast32BitUnsigned;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + CreateSignedTransaction<Call<Self>> + pallet_dkg_metadata::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// ChainID for anchor edges
		type ChainId: Encode
			+ Decode
			+ Parameter
			+ AtLeast32BitUnsigned
			+ Default
			+ Copy
			+ ChainIdTrait;
		/// The identifier type for an offchain worker.
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;
		/// Max number of signed proposal submissions per batch;
		#[pallet::constant]
		type MaxSubmissionsPerBatch: Get<u16>;
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
		ChainIdType<T::ChainId>,
		Blake2_128Concat,
		DKGPayloadKey,
		Proposal,
	>;

	/// All signed proposals.
	#[pallet::storage]
	#[pallet::getter(fn signed_proposals)]
	pub type SignedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		ChainIdType<T::ChainId>,
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
		},
		/// Event When a Proposal Gets Signed by DKG.
		ProposalSigned {
			/// The Target EVM chain ID.
			chain_id: ChainIdType<T::ChainId>,
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
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
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"offchain worker result: {:?}",
				res
			);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(<T as Config>::WeightInfo::submit_signed_proposals(props.len() as u32))]
		#[frame_support::transactional]
		pub fn submit_signed_proposals(
			origin: OriginFor<T>,
			props: Vec<Proposal>,
		) -> DispatchResultWithPostInfo {
			let sender = ensure_signed(origin)?;

			ensure!(
				props.len() <= T::MaxSubmissionsPerBatch::get() as usize,
				Error::<T>::ProposalsLengthOverflow
			);

			// log the caller, and the props.
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: props: {:?} by {:?}",
				&props,
				sender
			);

			for prop in &props {
				if let Proposal::Signed { kind, data, signature } = prop {
					let result =
						ensure_signed_by_dkg::<pallet_dkg_metadata::Pallet<T>>(signature, data)
							.map_err(|_| Error::<T>::ProposalSignatureInvalid);
					match result {
						Ok(_) => {
							// Do nothing, it is all good.
						},
						Err(_e) => {
							// this is a bad signature.
							// we emit it as an event.
							Self::deposit_event(Event::InvalidProposalSignature {
								kind: kind.clone(),
								data: data.clone(),
								invalid_signature: signature.clone(),
							});
							frame_support::log::error!(
								target: "dkg_proposal_handler",
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
					frame_support::log::debug!(
						target: "dkg_proposal_handler",
						"submit_signed_proposal: data: {:?}, signature: {:?}",
						data,
						signature
					);

					let prop = prop.clone();

					Self::handle_signed_proposal(prop)?;

					continue
				}

				Err(Error::<T>::ProposalSignatureInvalid)?;
			}

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
			if let Proposal::Unsigned { kind: _, data: _ } = &prop {
				match decode_proposal(&prop) {
					Ok((chain_id, key)) => {
						UnsignedProposalQueue::<T>::insert(chain_id, key, prop.clone());
						return Ok(().into())
					},
					Err(_) => return Err(Error::<T>::ProposalFormatInvalid)?,
				}
			}

			Err(Error::<T>::ProposalFormatInvalid)?
		}
	}
}

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	fn handle_unsigned_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		let proposal = Proposal::Unsigned { data: proposal, kind: ProposalKind::AnchorUpdate };
		if let Ok((chain_id, key)) = decode_proposal(&proposal).map(Into::into) {
			UnsignedProposalQueue::<T>::insert(chain_id, key, proposal);

			return Ok(())
		}

		Err(Error::<T>::ProposalFormatInvalid)?
	}

	fn handle_unsigned_proposer_set_update_proposal(
		proposal: Vec<u8>,
		_action: ProposalAction,
	) -> DispatchResult {
		let proposal = Proposal::Unsigned { data: proposal, kind: ProposalKind::ProposerSetUpdate };
		if let Ok((chain_id, key)) = decode_proposal(&proposal).map(Into::into) {
			UnsignedProposalQueue::<T>::insert(chain_id, key, proposal);

			return Ok(())
		}

		Err(Error::<T>::ProposalFormatInvalid)?
	}

	fn handle_unsigned_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		let unsigned_proposal =
			Proposal::Unsigned { data: proposal.encode(), kind: ProposalKind::Refresh };

		UnsignedProposalQueue::<T>::insert(
			ChainIdType::<T::ChainId>::EVM(T::ChainId::zero()),
			DKGPayloadKey::RefreshVote(proposal.nonce),
			unsigned_proposal,
		);

		Ok(().into())
	}

	fn handle_signed_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		UnsignedProposalQueue::<T>::remove(
			ChainIdType::<T::ChainId>::EVM(T::ChainId::zero()),
			DKGPayloadKey::RefreshVote(proposal.nonce),
		);

		Ok(().into())
	}

	fn handle_signed_proposal(prop: Proposal) -> DispatchResult {
		// Extract chain id and DKG key
		let (chain_id, payload_key) =
			decode_proposal(&prop).map_err(|_e| Error::<T>::ProposalFormatInvalid)?;
		// Log the chain id and nonce
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"submit_signed_proposal: chain_id: {:?}, payload_key: {:?}",
			chain_id,
			payload_key,
		);

		ensure!(
			UnsignedProposalQueue::<T>::contains_key(chain_id.clone(), payload_key),
			Error::<T>::ProposalDoesNotExists
		);
		// Log that proposal exist in the unsigned queue
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"submit_signed_proposal: proposal exist in the unsigned queue"
		);
		ensure!(
			Self::validate_proposal_signature(&prop.data(), &prop.signature()),
			Error::<T>::ProposalSignatureInvalid
		);
		// Log that the signature is valid
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"submit_signed_proposal: signature is valid"
		);
		// Update storage
		SignedProposals::<T>::insert(chain_id.clone(), payload_key, prop.clone());
		UnsignedProposalQueue::<T>::remove(chain_id.clone(), payload_key);
		// Emit event so frontend can react to it.
		Self::deposit_event(Event::<T>::ProposalSigned {
			chain_id,
			key: payload_key,
			data: prop.data().to_vec(),
			signature: prop.signature(),
		});

		Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***
	/// Returns the list of unsigned proposals with each chain id and payload key attached
	pub fn get_unsigned_proposals() -> Vec<((ChainIdType<T::ChainId>, DKGPayloadKey), Proposal)> {
		return UnsignedProposalQueue::<T>::iter()
			.map(|entry| ((entry.0, entry.1), entry.2.clone()))
			.collect()
	}

	/// Checks whether a signed proposal exists in the `SignedProposals` storage
	pub fn is_existing_proposal(prop: &Proposal) -> bool {
		if let Proposal::Signed { kind: _, data: _, .. } = prop {
			match dkg_runtime_primitives::handlers::decode_proposals::decode_proposal(prop) {
				Ok((chain_id, key)) => return !SignedProposals::<T>::contains_key(chain_id, key),
				Err(_) => return false,
			}
		}

		false
	}

	// *** Offchain worker methods ***

	/// Offchain worker function that submits signed proposals from the offchain storage on-chain
	///
	/// The function submits batches of signed proposals on-chain in batches of
	/// `T::MaxSubmissionsPerBatch`. Proposals are stored offchain and target specific block numbers
	/// for submission. This function polls all relevant proposals ready for submission at the
	/// current block number
	fn submit_signed_proposal_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let mut lock = StorageLock::<Time>::new(SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK);
		{
			let _guard = lock.lock();

			let signer = Signer::<T, <T as Config>::OffChainAuthId>::all_accounts();
			if !signer.can_sign() {
				return Err(
					"No local accounts available. Consider adding one via `author_insertKey` RPC.",
				)?
			}
			match Self::get_next_offchain_signed_proposal(block_number) {
				Ok(next_proposals) => {
					// We filter out all proposals that are already on chain
					let filtered_proposals = next_proposals
						.iter()
						.cloned()
						.filter(Self::is_existing_proposal)
						.collect::<Vec<_>>();

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
							frame_support::log::error!(
								target: "dkg_proposal_handler",
								"failure: failed to send unsigned transactiion to chain: {:?}",
								call,
							);
						} else {
							// log the result of the transaction submission
							frame_support::log::debug!(
								target: "dkg_proposal_handler",
								"Submitted unsigned transaction for signed proposal: {:?}",
								call,
							);
						}
					}
				},
				Err(e) => {
					// log the error
					frame_support::log::warn!(
						target: "dkg_proposal_handler",
						"Failed to get next signed proposal: {}",
						e
					);
				},
			};
			return Ok(())
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
					frame_support::log::debug!(
						target: "dkg_proposal_handler",
						"Offchain signed proposals: {:?}",
						prop_wrapper.proposals
					);
					// log how many proposal batches are left
					frame_support::log::debug!(
						target: "dkg_proposal_handler",
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
						.cloned()
						.flatten()
						.collect::<Vec<_>>();
					// then we need to keep only the batches that are not yet submitted
					prop_wrapper.proposals.retain(|(_, submit_at)| *submit_at > block_number);
					Ok(prop_wrapper)
				},
				Ok(None) => Err("No signed proposals key stored"),
				Err(e) => {
					// log the error
					frame_support::log::warn!(
						target: "dkg_proposal_handler",
						"Failed to read offchain signed proposals: {:?}",
						e
					);
					Err("Error decoding offchain signed proposals")
				},
			}
		});

		if res.is_err() || all_proposals.is_empty() {
			return Err("Unable to get next proposal batch")
		}

		Ok(all_proposals)
	}

	// *** Validation methods ***

	fn validate_proposal_signature(data: &Vec<u8>, signature: &Vec<u8>) -> bool {
		dkg_runtime_primitives::utils::validate_ecdsa_signature(data, signature)
	}

	// *** Utility methods ***

	#[cfg(feature = "runtime-benchmarks")]
	pub fn signed_proposals_len() -> usize {
		SignedProposals::<T>::iter_keys().collect::<Vec<_>>().len()
	}
}
