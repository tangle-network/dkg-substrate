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
use dkg_runtime_primitives::{
	handlers::decode_proposals::{decode_proposal_header, decode_proposal_identifier},
	traits::OnAuthoritySetChangeHandler,
	ProposalHandlerTrait, ProposalNonce, ResourceId, TypedChainId,
};
use frame_support::{
	pallet_prelude::{ensure, DispatchResultWithPostInfo},
	traits::{EnsureOrigin, EstimateNextSessionRotation, Get},
	BoundedVec,
};

use sp_runtime::{traits::Convert, RuntimeAppPublic};
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

	use dkg_runtime_primitives::{
		proposal::{Proposal, ProposalKind},
		ProposalNonce,
	};
	use frame_support::{
		dispatch::{fmt::Debug, DispatchResultWithPostInfo},
		pallet_prelude::*,
	};
	use frame_system::pallet_prelude::*;

	pub type ProposalOf<T> = Proposal<<T as Config>::MaxProposalLength>;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config: frame_system::Config {
		/// The overarching RuntimeEvent type.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Origin used to administer the pallet
		type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

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

		/// The handler for proposals
		type ProposalHandler: ProposalHandlerTrait<MaxProposalLength = Self::MaxProposalLength>;

		/// The identifier for this chain.
		/// This must be unique and must not collide with existing IDs within a
		/// set of bridged chains.
		#[pallet::constant]
		type ChainIdentifier: Get<TypedChainId>;

		#[pallet::constant]
		type ProposalLifetime: Get<Self::BlockNumber>;

		/// The session period
		#[pallet::constant]
		type Period: Get<Self::BlockNumber>;

		/// The max votes to store for for and against
		#[pallet::constant]
		type MaxVotes: Get<u32> + TypeInfo + Clone;

		/// The max resources that can be stored in storage
		#[pallet::constant]
		type MaxResources: Get<u32> + TypeInfo;

		/// The max proposers that can be stored in storage
		#[pallet::constant]
		type MaxProposers: Get<u32> + TypeInfo;

		/// The size of an external proposer account (i.e. 64-byte Ethereum public key)
		#[pallet::constant]
		type VotingKeySize: Get<u32> + Debug + Clone + Eq + PartialEq + PartialOrd + Ord + TypeInfo;

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

	/// Tracks current proposer set
	#[pallet::storage]
	#[pallet::getter(fn proposers)]
	pub type Proposers<T: Config> =
		StorageValue<_, BoundedVec<T::AccountId, T::MaxProposers>, ValueQuery>;

	/// Tracks current ECDSA voting keys for each validator
	pub type VotingKey<T> = BoundedVec<u8, <T as Config>::VotingKeySize>;
	pub type VotingKeyTuple<T> = (<T as frame_system::Config>::AccountId, VotingKey<T>);
	pub type VoterList<T> = BoundedVec<VotingKeyTuple<T>, <T as Config>::MaxProposers>;
	#[pallet::storage]
	#[pallet::getter(fn external_proposer_accounts)]
	pub type VotingKeys<T: Config> =
		StorageValue<_, BoundedVec<(T::AccountId, VotingKey<T>), T::MaxProposers>, ValueQuery>;

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
		(ProposalNonce, ProposalOf<T>),
		ProposalVotes<T::AccountId, T::BlockNumber, T::MaxVotes>,
	>;

	/// Utilized by the bridge software to map resource IDs to actual methods
	#[pallet::storage]
	#[pallet::getter(fn resources)]
	pub type Resources<T: Config> =
		StorageMap<_, Blake2_256, ResourceId, BoundedVec<u8, T::MaxResources>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event<T: Config> {
		/// Vote threshold has changed (new_threshold)
		ProposerThresholdChanged { new_threshold: u32 },
		/// Chain now available for transfers (chain_id)
		ChainWhitelisted { chain_id: TypedChainId },
		/// Vote submitted in favour of proposal
		VoteFor {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
			who: T::AccountId,
		},
		/// Vot submitted against proposal
		VoteAgainst {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
			who: T::AccountId,
		},
		/// Voting successful for a proposal
		ProposalApproved {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
		},
		/// Voting rejected a proposal
		ProposalRejected {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
		},
		/// Execution of call succeeded
		ProposalSucceeded {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
		},
		/// Execution of call failed
		ProposalFailed {
			kind: ProposalKind,
			src_chain_id: TypedChainId,
			proposal_nonce: ProposalNonce,
		},
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
		/// Proposer Count is Zero
		ProposerCountIsZero,
		/// Input is out of bounds
		OutOfBounds,
		/// Invalid proposal
		InvalidProposal,
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		/// Typed ChainId (chain type, chain id)
		pub initial_chain_ids: Vec<[u8; 6]>,
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
			for bytes in self.initial_chain_ids.iter() {
				let mut chain_id_bytes = [0u8; TypedChainId::LENGTH];
				let f = 0;
				let t = f + TypedChainId::LENGTH;
				chain_id_bytes.copy_from_slice(&bytes[f..t]);
				let chain_id = TypedChainId::from(chain_id_bytes);
				ChainNonces::<T>::insert(chain_id, ProposalNonce::from(0));
			}
			for (r_id, r_data) in self.initial_r_ids.iter() {
				let bounded_input: BoundedVec<_, _> =
					r_data.clone().try_into().expect("Genesis resources is too large");
				Resources::<T>::insert(*r_id, bounded_input);
			}

			let bounded_proposers: BoundedVec<T::AccountId, T::MaxProposers> = self
				.initial_proposers
				.clone()
				.try_into()
				.expect("Genesis proposers is too large");
			Proposers::<T>::put(bounded_proposers);
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
		#[pallet::weight(<T as Config>::WeightInfo::set_threshold())]
		#[pallet::call_index(0)]
		pub fn set_threshold(origin: OriginFor<T>, threshold: u32) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::set_proposer_threshold(threshold)
		}

		/// Stores a method name on chain under an associated resource ID.
		///
		/// # <weight>
		/// - O(1) write
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::set_resource())]
		#[pallet::call_index(1)]
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
		#[pallet::call_index(2)]
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
		#[pallet::call_index(3)]
		pub fn whitelist_chain(
			origin: OriginFor<T>,
			chain_id: TypedChainId,
		) -> DispatchResultWithPostInfo {
			Self::ensure_admin(origin)?;
			Self::whitelist(chain_id)
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
		#[pallet::weight(<T as Config>::WeightInfo::acknowledge_proposal())]
		#[pallet::call_index(4)]
		pub fn acknowledge_proposal(
			origin: OriginFor<T>,
			prop: ProposalOf<T>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			let proposal_ident =
				decode_proposal_identifier(&prop).map_err(|_| Error::<T>::InvalidProposal)?;
			let proposal_header =
				decode_proposal_header(prop.data()).map_err(|_| Error::<T>::InvalidProposal)?;

			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(
				Self::chain_whitelisted(proposal_ident.typed_chain_id),
				Error::<T>::ChainNotWhitelisted
			);
			ensure!(
				Self::resource_exists(proposal_header.resource_id),
				Error::<T>::ResourceDoesNotExist
			);

			Self::vote_for(who, proposal_header.nonce, proposal_ident.typed_chain_id, &prop)
		}

		/// Commits a vote against a provided proposal.
		///
		/// # <weight>
		/// - Fixed, since execution of proposal should not be included
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::reject_proposal())]
		#[pallet::call_index(5)]
		pub fn reject_proposal(
			origin: OriginFor<T>,
			prop: ProposalOf<T>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			let proposal_ident =
				decode_proposal_identifier(&prop).map_err(|_| Error::<T>::InvalidProposal)?;
			let proposal_header =
				decode_proposal_header(prop.data()).map_err(|_| Error::<T>::InvalidProposal)?;

			ensure!(Self::is_proposer(&who), Error::<T>::MustBeProposer);
			ensure!(
				Self::chain_whitelisted(proposal_ident.typed_chain_id),
				Error::<T>::ChainNotWhitelisted
			);
			ensure!(
				Self::resource_exists(proposal_header.resource_id),
				Error::<T>::ResourceDoesNotExist
			);

			Self::vote_against(who, proposal_header.nonce, proposal_ident.typed_chain_id, &prop)
		}

		/// Evaluate the state of a proposal given the current vote threshold.
		///
		/// A proposal with enough votes will be either executed or cancelled,
		/// and the status will be updated accordingly.
		///
		/// # <weight>
		/// - weight of proposed call, regardless of whether execution is performed
		/// # </weight>
		#[pallet::weight(<T as Config>::WeightInfo::eval_vote_state(prop.data().len() as u32))]
		#[pallet::call_index(6)]
		pub fn eval_vote_state(
			origin: OriginFor<T>,
			nonce: ProposalNonce,
			src_chain_id: TypedChainId,
			prop: ProposalOf<T>,
		) -> DispatchResultWithPostInfo {
			ensure_signed(origin)?;

			Self::try_resolve_proposal(nonce, src_chain_id, &prop)
		}
	}
}

impl<T: Config> Pallet<T> {
	// *** Utility methods ***

	pub fn ensure_admin(o: T::RuntimeOrigin) -> DispatchResultWithPostInfo {
		T::AdminOrigin::ensure_origin(o)?;
		Ok(().into())
	}

	/// Checks if who is a proposer
	pub fn is_proposer(who: &T::AccountId) -> bool {
		Self::proposers().contains(who)
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
		ensure!(threshold <= Proposers::<T>::get().len() as u32, Error::<T>::InvalidThreshold);
		ProposerThreshold::<T>::put(threshold);
		Self::deposit_event(Event::ProposerThresholdChanged { new_threshold: threshold });
		Ok(().into())
	}

	/// Register a method for a resource Id, enabling associated transfers
	pub fn register_resource(id: ResourceId, method: Vec<u8>) -> DispatchResultWithPostInfo {
		let bounded_method: BoundedVec<_, _> =
			method.try_into().map_err(|_| Error::<T>::OutOfBounds)?;
		Resources::<T>::insert(id, bounded_method);
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

	// *** Proposal voting and execution methods ***

	/// Commits a vote for a proposal. If the proposal doesn't exist it will be
	/// created.
	fn commit_vote(
		who: T::AccountId,
		nonce: ProposalNonce,
		src_chain_id: TypedChainId,
		prop: &ProposalOf<T>,
		in_favour: bool,
	) -> DispatchResultWithPostInfo {
		let now = <frame_system::Pallet<T>>::block_number();
		let mut votes = match Votes::<T>::get(src_chain_id, (nonce, prop.clone())) {
			Some(v) => v,
			None => ProposalVotes::<
				<T as frame_system::Config>::AccountId,
				<T as frame_system::Config>::BlockNumber,
				<T as Config>::MaxVotes,
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
			votes.votes_for.try_push(who.clone()).map_err(|_| Error::<T>::OutOfBounds)?;
			Self::deposit_event(Event::VoteFor {
				src_chain_id,
				proposal_nonce: nonce,
				kind: prop.kind(),
				who,
			});
		} else {
			votes.votes_against.try_push(who.clone()).map_err(|_| Error::<T>::OutOfBounds)?;
			Self::deposit_event(Event::VoteAgainst {
				src_chain_id,
				proposal_nonce: nonce,
				kind: prop.kind(),
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
		prop: &ProposalOf<T>,
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
				ProposalStatus::Rejected => Self::cancel_execution(src_chain_id, nonce, prop),
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
		prop: &ProposalOf<T>,
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
		prop: &ProposalOf<T>,
	) -> DispatchResultWithPostInfo {
		Self::commit_vote(who, nonce, src_chain_id, prop, false)?;
		Self::try_resolve_proposal(nonce, src_chain_id, prop)
	}

	/// Execute the proposal and signals the result as an RuntimeEvent
	fn finalize_execution(
		src_chain_id: TypedChainId,
		nonce: ProposalNonce,
		prop: &ProposalOf<T>,
	) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalApproved {
			src_chain_id,
			proposal_nonce: nonce,
			kind: prop.kind(),
		});
		T::ProposalHandler::handle_unsigned_proposal(prop.clone())?;
		Self::deposit_event(Event::ProposalSucceeded {
			src_chain_id,
			proposal_nonce: nonce,
			kind: prop.kind(),
		});
		Ok(().into())
	}

	/// Cancels a proposal.
	fn cancel_execution(
		src_chain_id: TypedChainId,
		nonce: ProposalNonce,
		prop: &ProposalOf<T>,
	) -> DispatchResultWithPostInfo {
		Self::deposit_event(Event::ProposalRejected {
			src_chain_id,
			proposal_nonce: nonce,
			kind: prop.kind(),
		});
		Ok(().into())
	}
}

impl<T: Config>
	OnAuthoritySetChangeHandler<T::AccountId, dkg_runtime_primitives::AuthoritySetId, T::DKGId>
	for Pallet<T>
{
	/// Called when the authority set has changed.
	///
	/// On new authority sets, we need to:
	/// -
	fn on_authority_set_changed(authorities: &[T::AccountId], authority_ids: &[T::DKGId]) {
		// Get the new external accounts for the new authorities by converting
		// their DKGIds to data meant for merkle tree insertion (i.e. Ethereum addresses)
		let new_external_accounts = authority_ids
			.iter()
			.map(|id| T::DKGAuthorityToMerkleLeaf::convert(id.clone()))
			.map(|id| {
				let bounded_external_account: VotingKey<T> =
					id.try_into().expect("External account outside limits!");
				bounded_external_account
			})
			.collect::<Vec<_>>();
		ProposerCount::<T>::put(authorities.len() as u32);
		let bounded_proposers: BoundedVec<T::AccountId, T::MaxProposers> =
			authorities.to_vec().try_into().expect("Too many authorities!");
		Proposers::<T>::put(bounded_proposers);
		let bounded_external_accounts: VoterList<T> = authorities
			.iter()
			.cloned()
			.zip(new_external_accounts)
			.collect::<Vec<_>>()
			.try_into()
			.expect("Too many external proposer accounts!");
		VotingKeys::<T>::put(bounded_external_accounts);
		Self::deposit_event(Event::<T>::ProposersReset { proposers: authorities.to_vec() });
	}
}

/// Convert DKG secp256k1 public keys into Ethereum addresses
pub struct DKGEcdsaToEthereumAddress;
impl Convert<dkg_runtime_primitives::crypto::AuthorityId, Vec<u8>> for DKGEcdsaToEthereumAddress {
	fn convert(a: dkg_runtime_primitives::crypto::AuthorityId) -> Vec<u8> {
		use k256::{ecdsa::VerifyingKey, elliptic_curve::sec1::ToEncodedPoint};
		let _x = VerifyingKey::from_sec1_bytes(sp_core::crypto::ByteArray::as_slice(&a));
		VerifyingKey::from_sec1_bytes(sp_core::crypto::ByteArray::as_slice(&a))
			.map(|pub_key| {
				// uncompress the key
				let uncompressed = pub_key.to_encoded_point(false);
				// convert to ETH address
				sp_io::hashing::keccak_256(&uncompressed.as_bytes()[1..])[12..].to_vec()
			})
			.map_err(|_| {
				log::error!(target: "runtime::dkg_proposals", "Invalid DKG PublicKey format!");
			})
			.unwrap_or_default()
	}
}
