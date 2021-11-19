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

use frame_support::{
	traits::{Get, OneSessionHandler},
	Parameter,
};
use frame_system::offchain::{SendSignedTransaction, Signer};

use core::convert::TryFrom;
use sp_runtime::{
	generic::DigestItem,
	offchain::storage::StorageValueRef,
	traits::{IsMember, Member},
	RuntimeAppPublic,
};
use sp_std::{collections::btree_map::BTreeMap, prelude::*};

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
	use frame_system::{
		ensure_signed,
		offchain::{AppCrypto, CreateSignedTransaction},
		pallet_prelude::*,
	};

	#[pallet::config]
	pub trait Config: frame_system::Config + CreateSignedTransaction<Call<Self>> {
		/// Authority identifier type
		type DKGId: Member + Parameter + RuntimeAppPublic + Default + MaybeSerializeDeserialize;

		/// The identifier type for an offchain worker.
		type OffChainAuthorityId: AppCrypto<Self::Public, Self::Signature>;

		/// Listener for authority set changes
		type OnAuthoritySetChangeHandler: OnAuthoritySetChangeHandler<
			dkg_runtime_primitives::AuthoritySetId,
			Self::AccountId,
		>;

		#[pallet::constant]
		type GracePeriod: Get<Self::BlockNumber>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			let _res = Self::submit_public_key_onchain(block_number);
		}
	}

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

		#[pallet::weight(0)]
		pub fn submit_public_key(
			origin: OriginFor<T>,
			pub_key: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			let origin = ensure_signed(origin)?;

			let next_authorities = Self::next_authorities_accounts();

			ensure!(next_authorities.contains(&origin), Error::<T>::MustBeAQueuedAuthority);

			NextPublicKeys::<T>::insert(origin, pub_key);

			Self::vote_public_key();

			Ok(().into())
		}
	}

	/// Tracks public keys submitted by queued authorities
	#[pallet::storage]
	#[pallet::getter(fn next_public_keys)]
	pub type NextPublicKeys<T: Config> =
		StorageMap<_, Blake2_256, T::AccountId, Vec<u8>, ValueQuery>;

	/// Holds public key for next session
	#[pallet::storage]
	#[pallet::getter(fn next_public_key)]
	pub type NextPublicKey<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), ValueQuery>;

	/// Holds active public key for ongoing session
	#[pallet::storage]
	#[pallet::getter(fn active_public_key)]
	pub type ActivePublicKey<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), ValueQuery>;

	/// Holds public key for immedaiate past session
	#[pallet::storage]
	#[pallet::getter(fn proposers)]
	pub type PreviousPublicKey<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), ValueQuery>;

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

	/// Authority account ids set scheduled to be used with the next session
	#[pallet::storage]
	#[pallet::getter(fn next_authorities_accounts)]
	pub(super) type NextAuthoritiesAccounts<T: Config> =
		StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	/// Public key derived by local node in the keygen
	#[pallet::storage]
	#[pallet::getter(fn local_node_key)]
	pub(super) type LocalNodePubKey<T: Config> = StorageValue<_, Vec<u8>, ValueQuery>;

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
		MustBeAQueuedAuthority,
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

	pub fn vote_public_key() -> () {
		let mut dict: BTreeMap<Vec<u8>, Vec<T::AccountId>> = BTreeMap::new();
		let authority_accounts = Self::next_authorities_accounts();

		for origin in authority_accounts.iter() {
			let key = Self::next_public_keys(origin);
			let mut temp = dict.remove(&key).unwrap_or_default();
			temp.push(origin.clone());
			dict.insert(key.clone(), temp);
		}

		let thresh = Self::signature_threshold();

		for (key, accounts) in dict.iter() {
			if accounts.len() >= thresh as usize {
				NextPublicKey::<T>::put((Self::authority_set_id() + 1u64, key.clone()));

				for acc in &authority_accounts {
					if !accounts.contains(acc) {
						// TODO Slash account for posting a wrong key
					}
				}
			}
		}
	}

	pub fn set_local_pub_key(key: Vec<u8>) -> () {
		LocalNodePubKey::<T>::put(key)
	}

	fn change_authorities(
		new: Vec<T::DKGId>,
		queued: Vec<T::DKGId>,
		authorities_accounts: Vec<T::AccountId>,
		next_authorities_accounts: Vec<T::AccountId>,
	) {
		// As in GRANDPA, we trigger a validator set change only if the the validator
		// set has actually changed.
		if new != Self::authorities() {
			<Authorities<T>>::put(&new);

			let next_id = Self::authority_set_id() + 1u64;

			<T::OnAuthoritySetChangeHandler as OnAuthoritySetChangeHandler<
				dkg_runtime_primitives::AuthoritySetId,
				T::AccountId,
			>>::on_authority_set_changed(next_id, authorities_accounts);

			<AuthoritySetId<T>>::put(next_id);

			let log: DigestItem<T::Hash> = DigestItem::Consensus(
				DKG_ENGINE_ID,
				ConsensusLog::AuthoritiesChange {
					next_authorities: AuthoritySet { authorities: new, id: next_id },
					next_queued_authorities: AuthoritySet {
						authorities: queued.clone(),
						id: next_id + 1u64,
					},
				}
				.encode(),
			);
			<frame_system::Pallet<T>>::deposit_log(log);
		}

		<NextAuthorities<T>>::put(&queued);
		NextAuthoritiesAccounts::<T>::put(&next_authorities_accounts)
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

	fn submit_public_key_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let submitted = StorageValueRef::persistent(b"dkg-metadata::submitted");

		let res = submitted.mutate(|submitted| match submitted {
			Ok(Some(block)) if block_number < block + T::GracePeriod::get() => Err(()),
			_ => Ok(block_number),
		});

		let pub_key = Self::local_node_key();

		match res {
			Ok(_block_number) => {
				let signer = Signer::<T, T::OffChainAuthorityId>::all_accounts();
				if !signer.can_sign() {
					return Err(
							"No local accounts available. Consider adding one via `author_insertKey` RPC.",
						)?
				}

				if !pub_key.is_empty() {
					let _ = signer.send_signed_transaction(|_account| Call::submit_public_key {
						pub_key: pub_key.clone(),
					});

					LocalNodePubKey::<T>::kill();
				}

				return Ok(())
			},
			_ => return Err("Already submitted public key")?,
		}
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
			let mut queued_authority_account_ids = Vec::new();
			let next_authorities = validators
				.map(|(l, k)| {
					authority_account_ids.push(l.clone());
					k
				})
				.collect::<Vec<_>>();

			let next_queued_authorities = queued_validators
				.map(|(acc, k)| {
					queued_authority_account_ids.push(acc.clone());
					k
				})
				.collect::<Vec<_>>();

			Self::change_authorities(
				next_authorities.clone(),
				next_queued_authorities,
				authority_account_ids,
				queued_authority_account_ids,
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
