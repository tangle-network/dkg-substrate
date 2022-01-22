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
//! Add description #TODO
//!
//! ## Overview
//!
//!
//! ### Terminology
//!
//! ### Goals
//!
//! The DKG proposal system is designed to make the following
//! possible:
//!
//! * Define.
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
pub mod types;
pub mod utils;
use crate::types::{ProposalStatus, ProposalVotes};
use codec::{Decode, Encode, EncodeAppend, EncodeLike};
use dkg_runtime_primitives::{
	traits::OnAuthoritySetChangeHandler, ProposalHandlerTrait, ProposalNonce, ResourceId,
};
use frame_support::{
	pallet_prelude::{ensure, DispatchResultWithPostInfo},
	traits::{EnsureOrigin, Get},
};
use frame_system::{self as system, ensure_root};
pub use pallet::*;
use scale_info::TypeInfo;
use sp_runtime::{traits::AccountIdConversion, RuntimeDebug};
use sp_std::prelude::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod weights;
pub use weights::DKGProposalsWeight;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use crate::types::{ProposalVotes, DKG_DEFAULT_PROPOSER_THRESHOLD};
	use dkg_runtime_primitives::ProposalNonce;
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*, PalletId};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::AtLeast32Bit;
	use crate::weights::WeightInfo;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// Origin used to administer the pallet
		type AdminOrigin: EnsureOrigin<Self::Origin>;

		/// Proposed transaction blob proposal
		type Proposal: Parameter + EncodeLike + EncodeAppend + Into<Vec<u8>>;

		/// ChainID for anchor edges
		type ChainId: Encode
			+ Decode
			+ Parameter
			+ AtLeast32Bit
			+ MaybeSerializeDeserialize
			+ Default
			+ Copy;

		/// The identifier for this chain.
		/// This must be unique and must not collide with existing IDs within a
		/// set of bridged chains.
		#[pallet::constant]
		type ChainIdentifier: Get<Self::ChainId>;

		#[pallet::constant]
		type ProposalLifetime: Get<Self::BlockNumber>;

		#[pallet::constant]
		type DKGAccountId: Get<PalletId>;

		type ProposalHandler: ProposalHandlerTrait;

		type WeightInfo: WeightInfo;
	}

	/// The parameter maintainer who can change the parameters
	#[pallet::storage]
	#[pallet::getter(fn maintainer)]
	pub type Maintainer<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

	/// All whitelisted chains and their respective transaction counts
	#[pallet::storage]
	#[pallet::getter(fn chains)]
	pub type ChainNonces<T: Config> = StorageMap<_, Blake2_256, T::ChainId, ProposalNonce>;

	#[pallet::type_value]
	pub fn DefaultForProposerThreshold() -> u32 {
		DKG_DEFAULT_PROPOSER_THRESHOLD
	}

	/// Number of votes required for a proposal to execute
	#[pallet::storage]
	#[pallet::getter(fn proposer_threshold)]
	pub type ProposerThreshold<T: Config> =
		StorageValue<_, u32, ValueQuery, DefaultForProposerThreshold>;

	/// Tracks current proposer set
	#[pallet::storage]
	#[pallet::getter(fn proposers)]
	pub type Proposers<T: Config> = StorageMap<_, Blake2_256, T::AccountId, bool, ValueQuery>;

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
		T::ChainId,
		Blake2_256,
		(ProposalNonce, T::Proposal),
		ProposalVotes<T::AccountId, T::BlockNumber>,
	>;

	/// Utilized by the bridge software to map resource IDs to actual methods
	#[pallet::storage]
	#[pallet::getter(fn resources)]
	pub type Resources<T: Config> = StorageMap<_, Blake2_256, ResourceId, Vec<u8>>;

	#[pallet::storage]
	#[pallet::getter(fn approve_pending_proposals)]
	pub type ApprovedPendingProposals<T: Config> = StorageMap<
		_,
		Blake2_256,
		u8, // some priority over proposals in the queue: 0 is highest priority
		Vec<T::Proposal>,
	>;

	// Pallets use events to inform users when important changes are made.
	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event<T: Config> {
		/// Maintainer is set
		MaintainerSet { old_maintainer: Option<T::AccountId>, new_maintainer: T::AccountId },
		/// Vote threshold has changed (new_threshold)
		ProposerThresholdChanged { new_threshold: u32 },
		/// Chain now available for transfers (chain_id)
		ChainWhitelisted { chain_id: T::ChainId },
		/// Proposer added to set
		ProposerAdded { proposer_id: T::AccountId },
		/// Proposer removed from set
		ProposerRemoved { proposer_id: T::AccountId },
		/// Vote submitted in favour of proposal
		VoteFor { chain_id: T::ChainId, proposal_nonce: ProposalNonce, who: T::AccountId },
		/// Vot submitted against proposal
		VoteAgainst { chain_id: T::ChainId, proposal_nonce: ProposalNonce, who: T::AccountId },
		/// Voting successful for a proposal
		ProposalApproved { chain_id: T::ChainId, proposal_nonce: ProposalNonce },
		/// Voting rejected a proposal
		ProposalRejected { chain_id: T::ChainId, proposal_nonce: ProposalNonce },
		/// Execution of call succeeded
		ProposalSucceeded { chain_id: T::ChainId, proposal_nonce: ProposalNonce },
		/// Execution of call failed
		ProposalFailed { chain_id: T::ChainId, proposal_nonce: ProposalNonce },
		/// Proposers have been reset
		ProposersReset { proposers: Vec<T::AccountId> },
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
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_chain_ids: Vec<T::ChainId>,
		pub initial_r_ids: Vec<(ResourceId, Vec<u8>)>,
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
			for chain_id in self.initial_chain_ids.iter() {
				ChainNonces::<T>::insert(chain_id, ProposalNonce::default());
			}
			for (r_id, r_data) in self.initial_r_ids.iter() {
				Resources::<T>::insert(r_id, r_data.clone());
			}

			for proposer in self.initial_proposers.iter() {
				Proposers::<T>::insert(proposer, true);
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Sets the maintainer.
		#[pallet::weight(<T as Config>::WeightInfo::set_maintainer())]
		pub fn set_maintainer(
			origin: OriginFor<T>,
			new_maintainer: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let origin = ensure_signed(origin)?;
			// ensure parameter setter is the maintainer
			ensure!(Some(origin.clone()) == Self::maintainer(), Error::<T>::InvalidPermissions);
			// set the new maintainer
			Maintainer::<T>::try_mutate(|maintainer| {
				*maintainer = Some(new_maintainer.clone());
				Self::deposit_event(Event::MaintainerSet {
					old_maintainer: Some(origin),
					new_maintainer,
				});
				Ok(().into())
			})
		}

		// Forcefully set the maintainer.
		#[pallet::weight(<T as Config>::WeightInfo::force_set_maintainer())]
		pub fn force_set_maintainer(
			origin: OriginFor<T>,
			new_maintainer: T::AccountId,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			// set the new maintainer
			Maintainer::<T>::try_mutate(|maintainer| {
				let old_maintainer = maintainer.clone();
				*maintainer = Some(new_maintainer.clone());
				Self::deposit_event(Event::MaintainerSet { old_maintainer, new_maintainer });
				Ok(().into())
			})
		}

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
		#[pallet::weight(<T as Config>::WeightInfo::set_resource(id.len() as u32))]
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
		pub fn whitelist_chain(origin: OriginFor<T>, id: T::ChainId) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::whitelist(id)
		}

		/// Adds a new proposer to the proposer set.
		///
		/// # <weight>
		/// - O(1) lookup and insert
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::add_proposer())]
		pub fn add_proposer(origin: OriginFor<T>, v: T::AccountId) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::register_proposer(v)
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
		/// - weight of proposed call, regardless of whether execution is
		///   performed
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::acknowledge_proposal())]
		pub fn acknowledge_proposal(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_id: T::ChainId,
			r_id: ResourceId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(Self::chain_whitelisted(src_id), Error::<T>::ChainNotWhitelisted);
			ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

			Self::vote_for(who, nonce, src_id, prop)
		}

		/// Commits a vote against a provided proposal.
		///
		/// # <weight>
		/// - Fixed, since execution of proposal should not be included
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::reject_proposal())]
		pub fn reject_proposal(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_id: T::ChainId,
			r_id: ResourceId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(Self::chain_whitelisted(src_id), Error::<T>::ChainNotWhitelisted);
			ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

			Self::vote_against(who, nonce, src_id, prop)
		}

		/// Evaluate the state of a proposal given the current vote threshold.
		///
		/// A proposal with enough votes will be either executed or cancelled,
		/// and the status will be updated accordingly.
		///
		/// # <weight>
		/// - weight of proposed call, regardless of whether execution is
		///   performed
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::eval_vote_state())]
		pub fn eval_vote_state(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_id: T::ChainId,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			ensure_signed(origin)?;

			Self::try_resolve_proposal(nonce, src_id, prop)
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

	/// Provides an AccountId for the pallet.
	/// This is used both as an origin check and deposit/withdrawal account.
	pub fn account_id() -> T::AccountId {
		T::DKGAccountId::get().into_account()
	}

	/// Asserts if a resource is registered
	pub fn resource_exists(id: ResourceId) -> bool {
		return Resources::<T>::contains_key(id)
	}

	/// Checks if a chain exists as a whitelisted destination
	pub fn chain_whitelisted(id: T::ChainId) -> bool {
		return ChainNonces::<T>::contains_key(id)
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
	pub fn whitelist(id: T::ChainId) -> DispatchResultWithPostInfo {
		// Cannot whitelist this chain
		ensure!(id != T::ChainIdentifier::get(), Error::<T>::InvalidChainId);
		// Cannot whitelist with an existing entry
		ensure!(!Self::chain_whitelisted(id), Error::<T>::ChainAlreadyWhitelisted);
		ChainNonces::<T>::insert(&id, 0);
		Self::deposit_event(Event::ChainWhitelisted { chain_id: id });
		Ok(().into())
	}

	/// Adds a new proposer to the set
	pub fn register_proposer(proposer: T::AccountId) -> DispatchResultWithPostInfo {
		ensure!(!Self::is_proposer(&proposer), Error::<T>::ProposerAlreadyExists);
		Proposers::<T>::insert(&proposer, true);
		ProposerCount::<T>::mutate(|i| *i += 1);

		Self::deposit_event(Event::ProposerAdded { proposer_id: proposer });
		Ok(().into())
	}

	/// Removes a proposer from the set
	pub fn unregister_proposer(proposer: T::AccountId) -> DispatchResultWithPostInfo {
		ensure!(Self::is_proposer(&proposer), Error::<T>::ProposerInvalid);
		Proposers::<T>::remove(&proposer);
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
		src_id: T::ChainId,
		prop: T::Proposal,
		in_favour: bool,
	) -> DispatchResultWithPostInfo {
		let now = <frame_system::Pallet<T>>::block_number();
		let mut votes = match Votes::<T>::get(src_id, (nonce, prop.clone())) {
			Some(v) => v,
			None => {
				let mut v = ProposalVotes::default();
				v.expiry = now + T::ProposalLifetime::get();
				v
			},
		};

		// Ensure the proposal isn't complete and proposer hasn't already voted
		ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
		ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);
		ensure!(!votes.has_voted(&who), Error::<T>::ProposerAlreadyVoted);

		if in_favour {
			votes.votes_for.push(who.clone());
			Self::deposit_event(Event::VoteFor {
				chain_id: src_id,
				proposal_nonce: nonce,
				who: who.clone(),
			});
		} else {
			votes.votes_against.push(who.clone());
			Self::deposit_event(Event::VoteAgainst {
				chain_id: src_id,
				proposal_nonce: nonce,
				who: who.clone(),
			});
		}

		Votes::<T>::insert(src_id, (nonce, prop.clone()), votes.clone());

		Ok(().into())
	}

	/// Attempts to finalize or cancel the proposal if the vote count allows.
	fn try_resolve_proposal(
		nonce: ProposalNonce,
		src_id: T::ChainId,
		prop: T::Proposal,
	) -> DispatchResultWithPostInfo {
		if let Some(mut votes) = Votes::<T>::get(src_id, (nonce, prop.clone())) {
			let now = <frame_system::Pallet<T>>::block_number();
			ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
			ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);

			let status =
				votes.try_to_complete(ProposerThreshold::<T>::get(), ProposerCount::<T>::get());
			Votes::<T>::insert(src_id, (nonce, prop.clone()), votes.clone());

			match status {
				ProposalStatus::Approved => Self::finalize_execution(src_id, nonce, prop),
				ProposalStatus::Rejected => Self::cancel_execution(src_id, nonce),
				_ => Ok(().into()),
			}
		} else {
			Err(Error::<T>::ProposalDoesNotExist)?
		}
	}

	/// Commits a vote in favour of the proposal and executes it if the vote
	/// threshold is met.
	fn vote_for(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_id: T::ChainId,
		prop: T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::commit_vote(who, nonce, src_id, prop.clone(), true)?;
		Self::try_resolve_proposal(nonce, src_id, prop)
	}

	/// Commits a vote against the proposal and cancels it if more than
	/// (proposers.len() - threshold) votes against exist.
	fn vote_against(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_id: T::ChainId,
		prop: T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::commit_vote(who, nonce, src_id, prop.clone(), false)?;
		Self::try_resolve_proposal(nonce, src_id, prop)
	}

	/// Execute the proposal and signals the result as an event
	fn finalize_execution(
		src_id: T::ChainId,
		nonce: ProposalNonce,
		prop: T::Proposal,
	) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalApproved { chain_id: src_id, proposal_nonce: nonce });
		T::ProposalHandler::handle_unsigned_proposal(
			prop.into(),
			dkg_runtime_primitives::ProposalAction::Sign(0),
		)?;
		Self::deposit_event(Event::ProposalSucceeded { chain_id: src_id, proposal_nonce: nonce });
		Ok(().into())
	}

	/// Cancels a proposal.
	fn cancel_execution(src_id: T::ChainId, nonce: ProposalNonce) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalRejected { chain_id: src_id, proposal_nonce: nonce });
		Ok(().into())
	}
}

/// Simple ensure origin for the bridge account
#[derive(Encode, Decode, Clone, Eq, PartialEq, TypeInfo, RuntimeDebug)]
pub struct EnsureBridge<T>(sp_std::marker::PhantomData<T>);
impl<T: Config> EnsureOrigin<T::Origin> for EnsureBridge<T> {
	type Success = T::AccountId;

	fn try_origin(o: T::Origin) -> Result<Self::Success, T::Origin> {
		let bridge_id = T::DKGAccountId::get().into_account();
		o.into().and_then(|o| match o {
			system::RawOrigin::Signed(who) if who == bridge_id => Ok(bridge_id),
			r => Err(T::Origin::from(r)),
		})
	}

	/// Returns an outer origin capable of passing `try_origin` check.
	///
	/// ** Should be used for benchmarking only!!! **
	#[cfg(feature = "runtime-benchmarks")]
	fn successful_origin() -> T::Origin {
		T::Origin::from(frame_system::RawOrigin::Signed(T::DKGAccountId::get().into_account()))
	}
}

impl<T: Config> OnAuthoritySetChangeHandler<dkg_runtime_primitives::AuthoritySetId, T::AccountId>
	for Pallet<T>
{
	fn on_authority_set_changed(
		_authority_set_id: dkg_runtime_primitives::AuthoritySetId,
		authorities: Vec<T::AccountId>,
	) -> () {
		Proposers::<T>::remove_all(Some(Self::proposer_count()));
		for authority in &authorities {
			Proposers::<T>::insert(authority, true);
		}
		ProposerCount::<T>::put(authorities.len() as u32);
		Self::deposit_event(Event::<T>::ProposersReset { proposers: authorities });
	}
}
