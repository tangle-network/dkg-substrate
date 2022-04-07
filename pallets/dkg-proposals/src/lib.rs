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

//! # DKG Proposals Module
//!
//! A pallet to manage proposals that are submitted for signing by the DKG.
//!
//! ## Overview
//!
//! The DKG proposals pallet manages a governance system derived from
//! ChainSafe's ChainBridge Substrate pallet. It is designed as the first
//! layer in Webb's DKG governance system and is responsible for managing
//! proposal submission and voting for messages that are intended to be signed
//! by the DKG threshold signing protocol.
//!
//! The pallet implements a simple threshold voting system wherein proposers
//! propose messages to be signed. Once a threshold of votes over the same
//! proposal is met, the message is handled by a generic proposal handler.
//! This pallet is intended to be used in conjunction with [`pallet-dkg-proposal-handler`].
//!
//! ### Terminology
//!
//! - Proposer: A valid account that can submit and vote on proposals.
//! - Proposal: A message that is submitted, voted on, and eventually handled or rejected.
//! - ProposerSet: The merkle root of the smallest merkle tree containing the ordered proposers.
//!
//! ### Implementation
//!
//! The DKG proposal system combines a set of proposers, a generic proposal message,
//! and a threshold-voting system to build a simple governance system for "handling"
//! proposals. By "handling", we intend for successful proposals to be sent to
//! a secondary system that acts upon proposal data.
//!
//! In the Webb Protocol, the handler submits successful proposals to the DKG for signing.
//!
//! The proposers of the pallet are derived from the active authorities of the underlying
//! chain as well as any account added to the set using the `add_proposer` call. The
//! intention is for the set of proposers to grow larger than simply the authority set
//! of the chain without growing the signing set of the underlying DKG.
//!
//! Proposers are required to submit 2 types of keys: AccountId keys and ECDSA keys. The former
//! keys are used to propose and interact with the Substrate based chain integrating this pallet.
//! The latter are used to interoperate with EVM systems who utilize the proposers for auxiliary
//! protocols described below. This aligns non-authority proposers with authority proposers as well
//! since we expect authorities to have both types of keys registered for consensus and DKG
//! activities.
//!
//! The proposals of the system are generic and left to be handled by a proposal handler.
//! Currently, upon inspection of the `pallet-dkg-proposal-handler` module, the only valid
//! proposal that can be proposed and handled successfully is the Anchor Update proposal:
//! the proposal responsible for bridging different anchors together in the Webb Protocol.
//!
//! The system can be seen as a 2-stage oracle-like system wherein proposers vote on
//! events/messages they believe to be valid and, if successful, the DKG will sign such events.
//!
//! The system also supports accumulating proposers for off-chain auxiliary protocols that utilize
//! proposers for new activities. In the Webb Protocol, we use the proposers to backstop the system
//! against critical failures and provide an emergency fallback mechanism when the DKG fails to sign
//! messages. We create a merkle tree of active proposers are submit the merkle root and session
//! length to the DKG for signing so as to maintain the list of active proposers across the
//! protocol's execution. If at any point in the future the DKG fails to sign messages and stops
//! working, we can utilize this merkle root to allow proposers to vote to restart and transition
//! any external system relying on the DKG to a new state.
//!
//! ### Rewards
//!
//! Currently, there are no extra rewards integrated for proposers. This is a future feature.
//!
//! ## Related Modules
//!
//! * [`System`](https://github.com/paritytech/substrate/tree/master/frame/system)
//! * [`Support`](https://github.com/paritytech/substrate/tree/master/frame/support)
//! * [`DKG Proposal Handler`](../../pallet-dkg-proposal-handler)

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
pub mod mock;
#[cfg(test)]
mod tests;
pub mod types;
pub mod utils;
use codec::{EncodeAppend, EncodeLike};
use dkg_runtime_primitives::{
	traits::OnAuthoritySetChangeHandler, ProposalHandlerTrait, ProposalNonce, ResourceId,
	TypedChainId,
};
use frame_support::{
	pallet_prelude::{ensure, DispatchResultWithPostInfo},
	traits::{EnsureOrigin, EstimateNextSessionRotation, Get},
};
use frame_system::ensure_root;
use sp_io::hashing::keccak_256;
use sp_runtime::{
	traits::{Convert, Saturating},
	RuntimeAppPublic,
};
use sp_std::prelude::*;
use types::{ProposalStatus, ProposalVotes};

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod weights;
pub use weights::WebbWeight;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use crate::{
		types::{ProposalVotes, DKG_DEFAULT_PROPOSER_THRESHOLD},
		weights::WeightInfo,
	};
	use dkg_runtime_primitives::ProposalNonce;
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// Origin used to administer the pallet
		type AdminOrigin: EnsureOrigin<Self::Origin>;

		/// Proposed transaction blob proposal
		type Proposal: Parameter + EncodeLike + EncodeAppend + Into<Vec<u8>> + AsRef<[u8]>;

		/// Estimate next session rotation
		type NextSessionRotation: EstimateNextSessionRotation<Self::BlockNumber>;

		/// Authority identifier type
		type DKGId: Member + Parameter + RuntimeAppPublic + MaybeSerializeDeserialize;
		/// Convert DKG AuthorityId to a form that would end up in the Merkle Tree.
		///
		/// For instance for ECDSA (secp256k1) we want to store uncompressed public keys (65 bytes)
		/// and later to Ethereum Addresses (160 bits) to simplify using them on Ethereum chain,
		/// but the rest of the Substrate codebase is storing them compressed (33 bytes) for
		/// efficiency reasons.
		type DKGAuthorityToMerkleLeaf: Convert<Self::DKGId, Vec<u8>>;

		/// The identifier for this chain.
		/// This must be unique and must not collide with existing IDs within a
		/// set of bridged chains.
		#[pallet::constant]
		type ChainIdentifier: Get<TypedChainId>;

		#[pallet::constant]
		type ProposalLifetime: Get<Self::BlockNumber>;

		type ProposalHandler: ProposalHandlerTrait;

		type WeightInfo: WeightInfo;
	}

	/// All whitelisted chains and their respective transaction counts
	#[pallet::storage]
	#[pallet::getter(fn chains)]
	pub type ChainNonces<T: Config> = StorageMap<_, Blake2_256, TypedChainId, ProposalNonce>;

	#[pallet::type_value]
	pub fn DefaultForProposerThreshold() -> u32 {
		DKG_DEFAULT_PROPOSER_THRESHOLD
	}

	/// Number of votes required for a proposal to execute
	#[pallet::storage]
	#[pallet::getter(fn proposer_threshold)]
	pub type ProposerThreshold<T: Config> =
		StorageValue<_, u32, ValueQuery, DefaultForProposerThreshold>;

	/// Proposer Set Update Proposal Nonce
	#[pallet::storage]
	#[pallet::getter(fn proposer_set_update_proposal_nonce)]
	pub type ProposerSetUpdateProposalNonce<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// Tracks current proposer set
	#[pallet::storage]
	#[pallet::getter(fn proposers)]
	pub type Proposers<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, bool, ValueQuery>;

	/// Tracks current proposer set external accounts
	/// Currently meant to store Ethereum compatible 64-bytes ECDSA public keys
	#[pallet::storage]
	#[pallet::getter(fn external_proposer_accounts)]
	pub type ExternalProposerAccounts<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, Vec<u8>, ValueQuery>;

	/// Tracks the authorities that are proposers so we can properly update the proposer set
	/// across sessions and authority changes.
	#[pallet::storage]
	#[pallet::getter(fn authority_proposers)]
	pub type AuthorityProposers<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	/// Tracks current proposer set external accounts
	#[pallet::storage]
	#[pallet::getter(fn external_authority_proposer_accounts)]
	pub type ExternalAuthorityProposerAccounts<T: Config> =
		StorageValue<_, Vec<Vec<u8>>, ValueQuery>;

	/// Number of proposers in set
	#[pallet::storage]
	#[pallet::getter(fn proposer_count)]
	pub type ProposerCount<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// All known proposals.
	/// The key is the hash of the call and the deposit ID, to ensure it's
	/// unique.
	#[pallet::storage]
	#[pallet::getter(fn votes)]
	pub type Votes<T: Config> = StorageDoubleMap<
		_,
		Blake2_256,
		TypedChainId,
		Blake2_256,
		(ProposalNonce, T::Proposal),
		ProposalVotes<T::AccountId, T::BlockNumber>,
	>;

	/// Utilized by the bridge software to map resource IDs to actual methods
	#[pallet::storage]
	#[pallet::getter(fn resources)]
	pub type Resources<T: Config> = StorageMap<_, Blake2_256, ResourceId, Vec<u8>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event<T: Config> {
		/// Vote threshold has changed (new_threshold)
		ProposerThresholdChanged { new_threshold: u32 },
		/// Chain now available for transfers (chain_id)
		ChainWhitelisted { chain_id: TypedChainId },
		/// Proposer added to set
		ProposerAdded { proposer_id: T::AccountId },
		/// Proposer removed from set
		ProposerRemoved { proposer_id: T::AccountId },
		/// Vote submitted in favour of proposal
		VoteFor { chain_id: TypedChainId, proposal_nonce: ProposalNonce, who: T::AccountId },
		/// Vot submitted against proposal
		VoteAgainst { chain_id: TypedChainId, proposal_nonce: ProposalNonce, who: T::AccountId },
		/// Voting successful for a proposal
		ProposalApproved { chain_id: TypedChainId, proposal_nonce: ProposalNonce },
		/// Voting rejected a proposal
		ProposalRejected { chain_id: TypedChainId, proposal_nonce: ProposalNonce },
		/// Execution of call succeeded
		ProposalSucceeded { chain_id: TypedChainId, proposal_nonce: ProposalNonce },
		/// Execution of call failed
		ProposalFailed { chain_id: TypedChainId, proposal_nonce: ProposalNonce },
		/// Proposers have been reset
		AuthorityProposersReset { proposers: Vec<T::AccountId> },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Account does not have correct permissions
		InvalidPermissions,
		/// Proposer threshold not set
		ThresholdNotSet,
		/// Provided chain Id is not valid
		InvalidChainId,
		/// Proposer threshold cannot be 0
		InvalidThreshold,
		/// Interactions with this chain is not permitted
		ChainNotWhitelisted,
		/// Chain has already been enabled
		ChainAlreadyWhitelisted,
		/// Resource ID provided isn't mapped to anything
		ResourceDoesNotExist,
		/// Proposer already in set
		ProposerAlreadyExists,
		/// Provided accountId is not a proposer
		ProposerInvalid,
		/// Protected operation, must be performed by proposer
		MustBeProposer,
		/// Proposer has already submitted some vote for this proposal
		ProposerAlreadyVoted,
		/// A proposal with these parameters has already been submitted
		ProposalAlreadyExists,
		/// No proposal with the ID was found
		ProposalDoesNotExist,
		/// Cannot complete proposal, needs more votes
		ProposalNotComplete,
		/// Proposal has either failed or succeeded
		ProposalAlreadyComplete,
		/// Lifetime of proposal has been exceeded
		ProposalExpired,
		/// Proposer Count is Zero
		ProposerCountIsZero,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		/// Typed ChainId (chain type, chain id)
		pub initial_chain_ids: Vec<[u8; 6]>,
		pub initial_r_ids: Vec<([u8; 32], Vec<u8>)>,
		pub initial_proposers: Vec<T::AccountId>,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self {
				initial_chain_ids: Default::default(),
				initial_r_ids: Default::default(),
				initial_proposers: Default::default(),
			}
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			for bytes in self.initial_chain_ids.iter() {
				let mut chain_id_bytes = [0u8; TypedChainId::LENGTH];
				let f = 0;
				let t = f + TypedChainId::LENGTH;
				chain_id_bytes.copy_from_slice(&bytes[f..t]);
				let chain_id = TypedChainId::from(chain_id_bytes);
				ChainNonces::<T>::insert(chain_id, ProposalNonce::from(0));
			}
			for (r_id, r_data) in self.initial_r_ids.iter() {
				Resources::<T>::insert(ResourceId::from(*r_id), r_data.clone());
			}

			for proposer in self.initial_proposers.iter() {
				Proposers::<T>::insert(proposer, true);
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Sets the vote threshold for proposals.
		///
		/// This threshold is used to determine how many votes are required
		/// before a proposal is executed.
		///
		/// # <weight>
		/// - O(1) lookup and insert
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::set_threshold(*threshold))]
		pub fn set_threshold(origin: OriginFor<T>, threshold: u32) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::set_proposer_threshold(threshold)
		}

		/// Stores a method name on chain under an associated resource ID.
		///
		/// # <weight>
		/// - O(1) write
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::set_resource(method.len() as u32))]
		pub fn set_resource(
			origin: OriginFor<T>,
			id: ResourceId,
			method: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::register_resource(id, method)
		}

		/// Removes a resource ID from the resource mapping.
		///
		/// After this call, bridge transfers with the associated resource ID
		/// will be rejected.
		///
		/// # <weight>
		/// - O(1) removal
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::remove_resource())]
		pub fn remove_resource(origin: OriginFor<T>, id: ResourceId) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::unregister_resource(id)
		}

		/// Enables a chain ID as a source or destination for a bridge transfer.
		///
		/// # <weight>
		/// - O(1) lookup and insert
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::whitelist_chain())]
		pub fn whitelist_chain(
			origin: OriginFor<T>,
			chain_id: TypedChainId,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::whitelist(chain_id)
		}

		/// Adds a new proposer to the proposer set.
		///
		/// # <weight>
		/// - O(1) lookup and insert
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::add_proposer())]
		pub fn add_proposer(
			origin: OriginFor<T>,
			native_account: T::AccountId,
			external_account: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::register_proposer(native_account, external_account)
		}

		/// Removes an existing proposer from the set.
		///
		/// # <weight>
		/// - O(1) lookup and removal
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::remove_proposer())]
		pub fn remove_proposer(
			origin: OriginFor<T>,
			v: T::AccountId,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::unregister_proposer(v)
		}

		/// Commits a vote in favour of the provided proposal.
		///
		/// If a proposal with the given nonce and source chain ID does not
		/// already exist, it will be created with an initial vote in favour
		/// from the caller.
		///
		/// # <weight>
		/// - weight of proposed call, regardless of whether execution is performed
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::acknowledge_proposal(prop.as_ref().len().try_into().unwrap()))]
		pub fn acknowledge_proposal(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_chain_id: TypedChainId,
			r_id: ResourceId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(Self::chain_whitelisted(src_chain_id), Error::<T>::ChainNotWhitelisted);
			ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

			Self::vote_for(who, nonce, src_chain_id, &prop)
		}

		/// Commits a vote against a provided proposal.
		///
		/// # <weight>
		/// - Fixed, since execution of proposal should not be included
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::reject_proposal(prop.as_ref().len().try_into().unwrap()))]
		pub fn reject_proposal(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_chain_id: TypedChainId,
			r_id: ResourceId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(Self::chain_whitelisted(src_chain_id), Error::<T>::ChainNotWhitelisted);
			ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

			Self::vote_against(who, nonce, src_chain_id, &prop)
		}

		/// Evaluate the state of a proposal given the current vote threshold.
		///
		/// A proposal with enough votes will be either executed or cancelled,
		/// and the status will be updated accordingly.
		///
		/// # <weight>
		/// - weight of proposed call, regardless of whether execution is performed
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::eval_vote_state(prop.as_ref().len().try_into().unwrap()))]
		pub fn eval_vote_state(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_chain_id: TypedChainId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			ensure_signed(origin)?;

			Self::try_resolve_proposal(nonce, src_chain_id, &prop)
		}
	}
}

impl<T: Config> Pallet<T> {
	// *** Utility methods ***

	pub fn ensure_admin(o: T::Origin) -> DispatchResultWithPostInfo {
		T::AdminOrigin::try_origin(o).map(|_| ()).or_else(ensure_root)?;
		Ok(().into())
	}

	/// Checks if who is a proposer
	pub fn is_proposer(who: &T::AccountId) -> bool {
		Self::proposers(who)
	}

	/// Asserts if a resource is registered
	pub fn resource_exists(id: ResourceId) -> bool {
		Resources::<T>::contains_key(id)
	}

	/// Checks if a chain exists as a whitelisted destination
	pub fn chain_whitelisted(chain_id: TypedChainId) -> bool {
		ChainNonces::<T>::contains_key(chain_id)
	}

	// *** Admin methods ***

	/// Set a new voting threshold
	pub fn set_proposer_threshold(threshold: u32) -> DispatchResultWithPostInfo {
		ensure!(threshold > 0, Error::<T>::InvalidThreshold);
		ProposerThreshold::<T>::put(threshold);
		Self::deposit_event(Event::ProposerThresholdChanged { new_threshold: threshold });
		Ok(().into())
	}

	/// Register a method for a resource Id, enabling associated transfers
	pub fn register_resource(id: ResourceId, method: Vec<u8>) -> DispatchResultWithPostInfo {
		Resources::<T>::insert(id, method);
		Ok(().into())
	}

	/// Removes a resource ID, disabling associated transfer
	pub fn unregister_resource(id: ResourceId) -> DispatchResultWithPostInfo {
		Resources::<T>::remove(id);
		Ok(().into())
	}

	/// Whitelist a chain ID for transfer
	pub fn whitelist(chain_id: TypedChainId) -> DispatchResultWithPostInfo {
		// Cannot whitelist this chain
		ensure!(chain_id != T::ChainIdentifier::get(), Error::<T>::InvalidChainId);
		// Cannot whitelist with an existing entry
		ensure!(!Self::chain_whitelisted(chain_id), Error::<T>::ChainAlreadyWhitelisted);
		ChainNonces::<T>::insert(chain_id, ProposalNonce::from(0));
		Self::deposit_event(Event::ChainWhitelisted { chain_id });
		Ok(().into())
	}

	/// Adds a new proposer to the set. Requires both an account ID and an ECDSA public key.
	pub fn register_proposer(
		proposer: T::AccountId,
		external_account: Vec<u8>,
	) -> DispatchResultWithPostInfo {
		ensure!(!Self::is_proposer(&proposer), Error::<T>::ProposerAlreadyExists);
		// Add the proposer to the set
		Proposers::<T>::insert(&proposer, true);
		// Add the proposer's public key to the set
		ExternalProposerAccounts::<T>::insert(&proposer, external_account);
		ProposerCount::<T>::mutate(|i| *i += 1);

		Self::deposit_event(Event::ProposerAdded { proposer_id: proposer });
		Ok(().into())
	}

	/// Removes a proposer from the set
	pub fn unregister_proposer(proposer: T::AccountId) -> DispatchResultWithPostInfo {
		ensure!(Self::is_proposer(&proposer), Error::<T>::ProposerInvalid);
		// Remove the proposer
		Proposers::<T>::remove(&proposer);
		// Remove the proposer's external account
		ExternalProposerAccounts::<T>::remove(&proposer);
		// Decrement the proposer count
		ProposerCount::<T>::mutate(|i| *i -= 1);
		Self::deposit_event(Event::ProposerRemoved { proposer_id: proposer });
		Ok(().into())
	}

	// *** Proposal voting and execution methods ***

	/// Commits a vote for a proposal. If the proposal doesn't exist it will be
	/// created.
	fn commit_vote(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_chain_id: TypedChainId,
		prop: &T::Proposal,
		in_favour: bool,
	) -> DispatchResultWithPostInfo {
		let now = <frame_system::Pallet<T>>::block_number();
		let mut votes = match Votes::<T>::get(src_chain_id, (nonce, prop.clone())) {
			Some(v) => v,
			None => ProposalVotes::<
				<T as frame_system::Config>::AccountId,
				<T as frame_system::Config>::BlockNumber,
			> {
				expiry: now + T::ProposalLifetime::get(),
				..Default::default()
			},
		};

		// Ensure the proposal isn't complete and proposer hasn't already voted
		ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
		ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);
		ensure!(!votes.has_voted(&who), Error::<T>::ProposerAlreadyVoted);

		if in_favour {
			votes.votes_for.push(who.clone());
			Self::deposit_event(Event::VoteFor {
				chain_id: src_chain_id,
				proposal_nonce: nonce,
				who,
			});
		} else {
			votes.votes_against.push(who.clone());
			Self::deposit_event(Event::VoteAgainst {
				chain_id: src_chain_id,
				proposal_nonce: nonce,
				who,
			});
		}

		Votes::<T>::insert(src_chain_id, (nonce, prop.clone()), votes.clone());

		Ok(().into())
	}

	/// Attempts to finalize or cancel the proposal if the vote count allows.
	fn try_resolve_proposal(
		nonce: ProposalNonce,
		src_chain_id: TypedChainId,
		prop: &T::Proposal,
	) -> DispatchResultWithPostInfo {
		if let Some(mut votes) = Votes::<T>::get(src_chain_id, (nonce, prop.clone())) {
			let now = <frame_system::Pallet<T>>::block_number();
			ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
			ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);

			let status =
				votes.try_to_complete(ProposerThreshold::<T>::get(), ProposerCount::<T>::get());
			Votes::<T>::insert(src_chain_id, (nonce, prop.clone()), votes.clone());

			match status {
				ProposalStatus::Approved => Self::finalize_execution(src_chain_id, nonce, prop),
				ProposalStatus::Rejected => Self::cancel_execution(src_chain_id, nonce),
				_ => Ok(().into()),
			}
		} else {
			Err(Error::<T>::ProposalDoesNotExist.into())
		}
	}

	/// Commits a vote in favour of the proposal and executes it if the vote
	/// threshold is met.
	fn vote_for(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_chain_id: TypedChainId,
		prop: &T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::commit_vote(who, nonce, src_chain_id, prop, true)?;
		Self::try_resolve_proposal(nonce, src_chain_id, prop)
	}

	/// Commits a vote against the proposal and cancels it if more than
	/// (proposers.len() - threshold) votes against exist.
	fn vote_against(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_chain_id: TypedChainId,
		prop: &T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::commit_vote(who, nonce, src_chain_id, prop, false)?;
		Self::try_resolve_proposal(nonce, src_chain_id, prop)
	}

	/// Execute the proposal and signals the result as an event
	fn finalize_execution(
		src_chain_id: TypedChainId,
		nonce: ProposalNonce,
		prop: &T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalApproved {
			chain_id: src_chain_id,
			proposal_nonce: nonce,
		});
		T::ProposalHandler::handle_unsigned_proposal(
			prop.clone().into(),
			dkg_runtime_primitives::ProposalAction::Sign(0),
		)?;
		Self::deposit_event(Event::ProposalSucceeded {
			chain_id: src_chain_id,
			proposal_nonce: nonce,
		});
		Ok(().into())
	}

	/// Cancels a proposal.
	fn cancel_execution(
		src_chain_id: TypedChainId,
		nonce: ProposalNonce,
	) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalRejected {
			chain_id: src_chain_id,
			proposal_nonce: nonce,
		});
		Ok(().into())
	}

	/// Creates the proposer set merkle tree and update proposal and submits the proposer for
	/// handling.
	///
	/// The proposer's external accounts (ECDSA keys) are used to create the proposer set. We
	/// hash the public keys into Ethereum compatible 20-byte addresses using the `keccak256` hash
	/// and insert these addresses into a minimally-sized merkle tree. This allows us to use the
	/// Ethereum origins (`msg.sender`) on EVMs to prove membership in the proposer set.
	///
	/// The proposal set update proposal is a message containing:
	/// - The merkle root of the ordered proposer set's Ethereum addresses.
	/// - The session length in milliseconds
	/// - The # of proposers accumulated in the merkle root
	/// - The nonce of the update proposal
	///
	/// The signed proposer set update is intended to be used to update the proposer set on
	/// other blockchains that need a fallback mechanism when the DKG is not available or needs
	/// to be fixed or changed.
	fn create_proposer_set_update() {
		// Merkleize the new proposer set
		let mut proposer_set_merkle_root = Self::get_proposer_set_tree_root();
		// Increment the nonce, we increment first because the nonce starts at 0
		let curr_proposal_nonce = Self::proposer_set_update_proposal_nonce();
		let new_proposal_nonce = curr_proposal_nonce.saturating_add(1u32);
		ProposerSetUpdateProposalNonce::<T>::put(new_proposal_nonce);
		// Get average session length
		let average_session_length_in_blocks: u64 =
			T::NextSessionRotation::average_session_length().try_into().unwrap_or_default();

		let average_millisecs_per_block: u64 =
			<T as pallet_timestamp::Config>::MinimumPeriod::get()
				.saturating_mul(2u32.into())
				.try_into()
				.unwrap_or_default();

		let average_session_length_in_millisecs =
			average_session_length_in_blocks.saturating_mul(average_millisecs_per_block);

		let num_of_proposers = Self::proposer_count();

		proposer_set_merkle_root
			.extend_from_slice(&average_session_length_in_millisecs.to_be_bytes());
		proposer_set_merkle_root.extend_from_slice(&num_of_proposers.to_be_bytes());
		proposer_set_merkle_root.extend_from_slice(&new_proposal_nonce.to_be_bytes());

		match T::ProposalHandler::handle_unsigned_proposer_set_update_proposal(
			proposer_set_merkle_root,
			dkg_runtime_primitives::ProposalAction::Sign(0),
		) {
			Ok(()) => {},
			Err(e) => {
				log::error!(
					target: "runtime::dkg_proposals",
					"Error creating proposer set update: {:?}", e
				);
			},
		}
	}

	/// Returns the leaves of the proposer set merkle tree.
	///
	/// It is expected that the size of the returned vector is a power of 2.
	pub fn pre_process_for_merkleize() -> Vec<Vec<u8>> {
		let height = Self::get_proposer_set_tree_height();
		let proposer_keys = ExternalProposerAccounts::<T>::iter_keys();
		// Check for each key that the proposer is valid (should return true)
		let mut base_layer: Vec<Vec<u8>> = proposer_keys
			.filter(|v| ExternalProposerAccounts::<T>::contains_key(v))
			.map(|x| keccak_256(&ExternalProposerAccounts::<T>::get(x)[..]).to_vec())
			.collect();
		// Pad base_layer to have length 2^height
		let two = 2;
		while base_layer.len() != two.saturating_pow(height.try_into().unwrap_or_default()) {
			base_layer.push(keccak_256(&[0u8]).to_vec());
		}
		base_layer
	}

	/// Computes the next layer of the merkle tree by hashing the previous layer.
	pub fn next_layer(curr_layer: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
		let mut layer_above: Vec<Vec<u8>> = Vec::new();
		let mut index = 0;
		while index < curr_layer.len() {
			let mut input_to_hash_as_vec: Vec<u8> = curr_layer[index].clone();
			input_to_hash_as_vec.extend_from_slice(&curr_layer[index + 1][..]);
			let input_to_hash_as_slice = &input_to_hash_as_vec[..];
			layer_above.push(keccak_256(input_to_hash_as_slice).to_vec());
			index += 2;
		}
		layer_above
	}

	// Returns the minimal height of the proposer set Merkle tree
	// Right now this takes O(log(size of proposer set)) time but can likely be reduced
	pub fn get_proposer_set_tree_height() -> u32 {
		if Self::proposer_count() == 1 {
			1
		} else {
			let two: u32 = 2;
			let mut h = 0;
			while two.saturating_pow(h) < Self::proposer_count() {
				h += 1;
			}
			h
		}
	}

	/// Computes the merkle root of the proposer set tree
	pub fn get_proposer_set_tree_root() -> Vec<u8> {
		let mut curr_layer = Self::pre_process_for_merkleize();
		let mut height = Self::get_proposer_set_tree_height();
		while height > 0 {
			curr_layer = Self::next_layer(curr_layer);
			height -= 1;
		}
		curr_layer[0].clone()
	}
}

impl<T: Config>
	OnAuthoritySetChangeHandler<T::AccountId, dkg_runtime_primitives::AuthoritySetId, T::DKGId>
	for Pallet<T>
{
	/// Called when the authority set has changed.
	///
	/// On new authority sets, we need to:
	/// - Remove the old authorities from the proposer set and their ECDSA keys from the external
	///   proposer accounts
	/// - Add the new authorities to the proposer set and their ECDSA keys to the external proposer
	///   accounts
	/// - Create a new proposer set update proposal by merkleizing the new proposer set
	/// - Submit the new proposet set update to the `pallet-dkg-proposal-handler`
	fn on_authority_set_changed(authorities: Vec<T::AccountId>, authority_ids: Vec<T::DKGId>) {
		// Get the new external accounts for the new authorities by converting
		// their DKGIds to data meant for merkle tree insertion (i.e. Ethereum addresses)
		let new_external_accounts = authority_ids
			.iter()
			.map(|id| T::DKGAuthorityToMerkleLeaf::convert(id.clone()))
			.collect::<Vec<_>>();
		// TODO: Get difference in list and optimise storage reads/writes
		// Remove old authorities and their external accounts from the list
		let old_authority_proposers = Self::authority_proposers();
		for old_authority_account in old_authority_proposers {
			ProposerCount::<T>::put(Self::proposer_count().saturating_sub(1));
			Proposers::<T>::remove(&old_authority_account);
			ExternalProposerAccounts::<T>::remove(&old_authority_account);
		}
		// Add new authorities and their external accounts to the list
		for (authority_account, external_account) in
			authorities.iter().zip(new_external_accounts.iter())
		{
			ProposerCount::<T>::put(Self::proposer_count().saturating_add(1));
			Proposers::<T>::insert(authority_account, true);
			ExternalProposerAccounts::<T>::insert(authority_account, external_account);
		}
		// Update the new authorities that are also proposers
		AuthorityProposers::<T>::put(authorities.clone());
		// Update the external accounts of the new authorities
		ExternalAuthorityProposerAccounts::<T>::put(new_external_accounts);
		Self::deposit_event(Event::<T>::AuthorityProposersReset { proposers: authorities });
		// Create the new proposer set merkle tree and update proposal
		Self::create_proposer_set_update();
	}
}

/// Convert DKG secp256k1 public keys into Ethereum addresses
pub struct DKGEcdsaToEthereum;
impl Convert<dkg_runtime_primitives::crypto::AuthorityId, Vec<u8>> for DKGEcdsaToEthereum {
	fn convert(a: dkg_runtime_primitives::crypto::AuthorityId) -> Vec<u8> {
		use k256::{ecdsa::VerifyingKey, elliptic_curve::sec1::ToEncodedPoint};
		use sp_core::crypto::ByteArray;
		let _x = VerifyingKey::from_sec1_bytes(
			dkg_runtime_primitives::crypto::AuthorityId::as_slice(&a),
		);
		VerifyingKey::from_sec1_bytes(dkg_runtime_primitives::crypto::AuthorityId::as_slice(&a))
			.map(|pub_key| {
				// uncompress the key
				let uncompressed = pub_key.to_encoded_point(false);
				// convert to ETH address
				sp_io::hashing::keccak_256(&uncompressed.as_bytes()[1..])[12..].to_vec()
			})
			.map_err(|_| {
				log::error!(target: "runtime::beefy", "Invalid BEEFY PublicKey format!");
			})
			.unwrap_or_default()
	}
}
