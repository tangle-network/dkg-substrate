// Copyright (C) 2020-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Encode;

use frame_support::{traits::OneSessionHandler, Parameter};

use core::convert::TryFrom;
use sp_runtime::{
	generic::DigestItem,
	traits::{IsMember, Member},
	RuntimeAppPublic,
};
use sp_std::prelude::*;

use dkg_runtime_primitives::{
	traits::OnAuthoritySetChangeHandler, AuthorityIndex, AuthoritySet, ConsensusLog, DKG_ENGINE_ID,
};

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Authority identifier type
		type DKGId: Member + Parameter + RuntimeAppPublic + Default + MaybeSerializeDeserialize;

		/// Listener for authority set changes
		type OnAuthoritySetChangeHandler: OnAuthoritySetChangeHandler<
			dkg_runtime_primitives::AuthoritySetId,
			Self::AccountId,
		>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn set_threshold(
			origin: OriginFor<T>,
			new_threshold: u16,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;
			ensure!(
				usize::from(new_threshold) <= Authorities::<T>::get().len(),
				Error::<T>::InvalidThreshold
			);
			// set the new maintainer
			SignatureThreshold::<T>::try_mutate(|threshold| {
				*threshold = new_threshold.clone();
				Ok(().into())
			})
		}
	}

	/// The current signature threshold (i.e. the `t` in t-of-n)
	#[pallet::storage]
	#[pallet::getter(fn signature_threshold)]
	pub(super) type SignatureThreshold<T: Config> = StorageValue<_, u16, ValueQuery>;

	/// The current authorities set
	#[pallet::storage]
	#[pallet::getter(fn authorities)]
	pub(super) type Authorities<T: Config> = StorageValue<_, Vec<T::DKGId>, ValueQuery>;

	/// The current authority set id
	#[pallet::storage]
	#[pallet::getter(fn authority_set_id)]
	pub(super) type AuthoritySetId<T: Config> =
		StorageValue<_, dkg_runtime_primitives::AuthoritySetId, ValueQuery>;

	/// Authorities set scheduled to be used with the next session
	#[pallet::storage]
	#[pallet::getter(fn next_authorities)]
	pub(super) type NextAuthorities<T: Config> = StorageValue<_, Vec<T::DKGId>, ValueQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub authorities: Vec<T::DKGId>,
		pub threshold: u32,
		pub authority_ids: Vec<T::AccountId>,
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Invalid threshold
		InvalidThreshold,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { authorities: Vec::new(), threshold: 0, authority_ids: Vec::new() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			Pallet::<T>::initialize_authorities(&self.authorities, &self.authority_ids);
			let sig_threshold = u16::try_from(self.authorities.len() / 2).unwrap() + 1;
			SignatureThreshold::<T>::put(sig_threshold);
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Return the current active DKG authority set.
	pub fn authority_set() -> AuthoritySet<T::DKGId> {
		AuthoritySet::<T::DKGId> { authorities: Self::authorities(), id: Self::authority_set_id() }
	}

	pub fn sig_threshold() -> u16 {
		Self::signature_threshold()
	}

	fn change_authorities(
		new: Vec<T::DKGId>,
		queued: Vec<T::DKGId>,
		authority_account_ids: Vec<T::AccountId>,
	) {
		// As in GRANDPA, we trigger a validator set change only if the the validator
		// set has actually changed.
		if new != Self::authorities() {
			<Authorities<T>>::put(&new);

			let next_id = Self::authority_set_id() + 1u64;
			<T::OnAuthoritySetChangeHandler as OnAuthoritySetChangeHandler<
				dkg_runtime_primitives::AuthoritySetId,
				T::AccountId,
			>>::on_authority_set_changed(next_id, authority_account_ids);
			<AuthoritySetId<T>>::put(next_id);

			let log: DigestItem<T::Hash> = DigestItem::Consensus(
				DKG_ENGINE_ID,
				ConsensusLog::AuthoritiesChange(AuthoritySet { authorities: new, id: next_id })
					.encode(),
			);
			<frame_system::Pallet<T>>::deposit_log(log);
		}

		<NextAuthorities<T>>::put(&queued);
	}

	fn initialize_authorities(authorities: &[T::DKGId], authority_account_ids: &[T::AccountId]) {
		if authorities.is_empty() {
			return
		}

		assert!(<Authorities<T>>::get().is_empty(), "Authorities are already initialized!");

		<Authorities<T>>::put(authorities);
		<AuthoritySetId<T>>::put(0);
		// Like `pallet_session`, initialize the next validator set as well.
		<NextAuthorities<T>>::put(authorities);
		<T::OnAuthoritySetChangeHandler as OnAuthoritySetChangeHandler<
			dkg_runtime_primitives::AuthoritySetId,
			T::AccountId,
		>>::on_authority_set_changed(0, authority_account_ids.to_vec());
	}
}

impl<T: Config> sp_runtime::BoundToRuntimeAppPublic for Pallet<T> {
	type Public = T::DKGId;
}

impl<T: Config> OneSessionHandler<T::AccountId> for Pallet<T> {
	type Key = T::DKGId;

	fn on_genesis_session<'a, I: 'a>(validators: I)
	where
		I: Iterator<Item = (&'a T::AccountId, T::DKGId)>,
	{
		let mut authority_account_ids = Vec::new();
		let authorities = validators
			.map(|(l, k)| {
				authority_account_ids.push(l.clone());
				k
			})
			.collect::<Vec<_>>();

		Self::initialize_authorities(&authorities, &authority_account_ids);
	}

	fn on_new_session<'a, I: 'a>(changed: bool, validators: I, queued_validators: I)
	where
		I: Iterator<Item = (&'a T::AccountId, T::DKGId)>,
	{
		if changed {
			let mut authority_account_ids = Vec::new();
			let next_authorities = validators
				.map(|(l, k)| {
					authority_account_ids.push(l.clone());
					k
				})
				.collect::<Vec<_>>();

			let next_queued_authorities = queued_validators.map(|(_, k)| k).collect::<Vec<_>>();

			Self::change_authorities(
				next_authorities.clone(),
				next_queued_authorities,
				authority_account_ids,
			);
		}
	}

	fn on_disabled(i: usize) {
		let log: DigestItem<T::Hash> = DigestItem::Consensus(
			DKG_ENGINE_ID,
			ConsensusLog::<T::DKGId>::OnDisabled(i as AuthorityIndex).encode(),
		);

		<frame_system::Pallet<T>>::deposit_log(log);
	}
}

impl<T: Config> IsMember<T::DKGId> for Pallet<T> {
	fn is_member(authority_id: &T::DKGId) -> bool {
		Self::authorities().iter().any(|id| id == authority_id)
	}
}
