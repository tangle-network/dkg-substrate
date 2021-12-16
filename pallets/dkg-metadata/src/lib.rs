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
	dispatch::DispatchResultWithPostInfo,
	traits::{EstimateNextSessionRotation, Get, OneSessionHandler},
	Parameter,
};
use frame_system::offchain::{SendSignedTransaction, Signer};

use sp_runtime::{
	generic::DigestItem,
	offchain::storage::StorageValueRef,
	traits::{IsMember, Member},
	Permill, RuntimeAppPublic,
};
use sp_std::{collections::btree_map::BTreeMap, convert::TryFrom, prelude::*};

use dkg_runtime_primitives::{
	traits::{GetDKGPublicKey, OnAuthoritySetChangeHandler},
	utils::{sr25519, to_slice_32, verify_signer_from_set},
	AggregatedPublicKeys, AuthorityIndex, AuthoritySet, ConsensusLog, AGGREGATED_PUBLIC_KEYS,
	AGGREGATED_PUBLIC_KEYS_AT_GENESIS, DKG_ENGINE_ID, OFFCHAIN_PUBLIC_KEY_SIG,
	SUBMIT_GENESIS_KEYS_AT, SUBMIT_KEYS_AT,
};

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{ensure, pallet_prelude::*};
	use frame_system::{
		ensure_signed,
		offchain::{AppCrypto, CreateSignedTransaction},
		pallet_prelude::*,
	};
	use sp_runtime::Permill;

	#[pallet::config]
	pub trait Config: frame_system::Config + CreateSignedTransaction<Call<Self>> {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// Authority identifier type
		type DKGId: Member + Parameter + RuntimeAppPublic + Default + MaybeSerializeDeserialize;

		/// The identifier type for an offchain worker.
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;

		/// Listener for authority set changes
		type OnAuthoritySetChangeHandler: OnAuthoritySetChangeHandler<
			dkg_runtime_primitives::AuthoritySetId,
			Self::AccountId,
		>;

		/// A type that gives allows the pallet access to the session progress
		type NextSessionRotation: EstimateNextSessionRotation<Self::BlockNumber>;

		/// Percentage session should have progressed for refresh to begin
		#[pallet::constant]
		type RefreshDelay: Get<Permill>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			let _res = Self::submit_genesis_public_key_onchain(block_number);
			let _res = Self::submit_next_public_key_onchain(block_number);
			let _res = Self::submit_public_key_signature_onchain(block_number);
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
				usize::from(new_threshold) < ((Authorities::<T>::get().len() / 2) + 1),
				Error::<T>::InvalidThreshold
			);
			// set the new maintainer
			SignatureThreshold::<T>::try_mutate(|threshold| {
				*threshold = new_threshold.clone();
				Ok(().into())
			})
		}

		#[pallet::weight(0)]
		pub fn set_refresh_delay(
			origin: OriginFor<T>,
			new_delay: u8,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			ensure!(new_delay <= 100, Error::<T>::InvalidRefreshDelay);

			// set the new delay
			RefreshDelay::<T>::put(Permill::from_percent(new_delay as u32));

			Ok(().into())
		}

		#[pallet::weight(0)]
		pub fn submit_public_key(
			origin: OriginFor<T>,
			keys_and_signatures: AggregatedPublicKeys,
		) -> DispatchResultWithPostInfo {
			let origin = ensure_signed(origin)?;

			let authorities = Self::current_authorities_accounts();

			ensure!(authorities.contains(&origin), Error::<T>::MustBeAnActiveAuthority);

			let dict = Self::process_public_key_submissions(keys_and_signatures, authorities);

			let threshold = Self::signature_threshold();

			let mut accepted = false;
			for (key, accounts) in dict.iter() {
				if accounts.len() >= threshold as usize {
					DKGPublicKey::<T>::put((Self::authority_set_id(), key.clone()));
					Self::deposit_event(Event::PublicKeySubmitted { pub_key: key.clone() });
					accepted = true;

					break
				}
			}

			if accepted {
				// TODO Do something about accounts that posted a wrong key
				return Ok(().into())
			}

			Err(Error::<T>::InvalidPublicKeys.into())
		}

		#[pallet::weight(0)]
		pub fn submit_next_public_key(
			origin: OriginFor<T>,
			keys_and_signatures: AggregatedPublicKeys,
		) -> DispatchResultWithPostInfo {
			let origin = ensure_signed(origin)?;

			let next_authorities = Self::next_authorities_accounts();

			ensure!(next_authorities.contains(&origin), Error::<T>::MustBeAQueuedAuthority);

			let dict = Self::process_public_key_submissions(keys_and_signatures, next_authorities);

			let threshold = Self::signature_threshold();

			let mut accepted = false;
			for (key, accounts) in dict.iter() {
				if accounts.len() >= threshold as usize {
					NextDKGPublicKey::<T>::put((Self::authority_set_id() + 1u64, key.clone()));
					Self::deposit_event(Event::NextPublicKeySubmitted { pub_key: key.clone() });
					accepted = true;

					break
				}
			}

			if accepted {
				// TODO Do something about accounts that posted a wrong key
				return Ok(().into())
			}

			Err(Error::<T>::InvalidPublicKeys.into())
		}

		#[pallet::weight(0)]
		pub fn submit_public_key_signature(
			origin: OriginFor<T>,
			signature: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			let origin = ensure_signed(origin)?;

			let authorities = Self::current_authorities_accounts();
			let used_signatures = Self::used_signatures();

			ensure!(authorities.contains(&origin), Error::<T>::MustBeAnActiveAuthority);

			ensure!(!used_signatures.contains(&signature), Error::<T>::UsedSignature);

			ensure!(signature != Vec::<u8>::default(), Error::<T>::InvalidSignature);

			Self::verify_pub_key_signature(origin, &signature)
		}
	}

	/// Public key Signatures for past sessions
	#[pallet::storage]
	#[pallet::getter(fn used_signatures)]
	pub type UsedSignatures<T: Config> = StorageValue<_, Vec<Vec<u8>>, ValueQuery>;

	/// Signature of the DKG public key for the next session
	#[pallet::storage]
	#[pallet::getter(fn next_public_key_signature)]
	pub type NextPublicKeySignature<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), OptionQuery>;

	/// Session progress required to kickstart refresh process
	#[pallet::storage]
	#[pallet::getter(fn refresh_delay)]
	pub type RefreshDelay<T: Config> = StorageValue<_, Permill, ValueQuery>;

	/// Holds public key for next session
	#[pallet::storage]
	#[pallet::getter(fn next_dkg_public_key)]
	pub type NextDKGPublicKey<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), OptionQuery>;

	/// Holds active public key for ongoing session
	#[pallet::storage]
	#[pallet::getter(fn dkg_public_key)]
	pub type DKGPublicKey<T: Config> =
		StorageValue<_, (dkg_runtime_primitives::AuthoritySetId, Vec<u8>), ValueQuery>;

	/// Signature of the current DKG public key
	#[pallet::storage]
	#[pallet::getter(fn public_key_signature)]
	pub type DKGPublicKeySignature<T: Config> = StorageValue<_, Vec<u8>, ValueQuery>;

	/// Holds public key for immediate past session
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

	/// Accounts for the current authorities
	#[pallet::storage]
	#[pallet::getter(fn current_authorities_accounts)]
	pub(super) type CurrentAuthoritiesAccounts<T: Config> =
		StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	/// Authority account ids scheduled for the next session
	#[pallet::storage]
	#[pallet::getter(fn next_authorities_accounts)]
	pub(super) type NextAuthoritiesAccounts<T: Config> =
		StorageValue<_, Vec<T::AccountId>, ValueQuery>;

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
		/// Must be queued  to become an authority
		MustBeAQueuedAuthority,
		/// Must be an an authority
		MustBeAnActiveAuthority,
		/// Refresh delay should be in the range of 0% - 100%
		InvalidRefreshDelay,
		/// Invalid public key submission
		InvalidPublicKeys,
		/// Already submitted a public key
		AlreadySubmittedPublicKey,
		/// Already submitted a public key signature
		AlreadySubmittedSignature,
		/// Used signature from past sessions
		UsedSignature,
		/// Invalid public key signature submission
		InvalidSignature,
	}

	// Pallets use events to inform users when important changes are made.
	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event<T: Config> {
		/// Current public key submitted
		PublicKeySubmitted { pub_key: Vec<u8> },
		/// Next public key submitted
		NextPublicKeySubmitted { pub_key: Vec<u8> },
		/// Next public key signature submitted
		NextPublicKeySignatureSubmitted { pub_key_sig: Vec<u8> },
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
			let sig_threshold = u16::try_from(self.authorities.len() / 2).unwrap() + 1;
			SignatureThreshold::<T>::put(sig_threshold);
			RefreshDelay::<T>::put(T::RefreshDelay::get());
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

	pub fn process_public_key_submissions(
		aggregated_keys: AggregatedPublicKeys,
		authorities: Vec<T::AccountId>,
	) -> BTreeMap<Vec<u8>, Vec<Vec<u8>>> {
		let mut dict: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();

		for (pub_key, signature) in aggregated_keys.keys_and_signatures {
			let maybe_signers = authorities
				.iter()
				.map(|x| {
					sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
						panic!("Failed to convert account id to sr25519 public key")
					}))
				})
				.collect::<Vec<sr25519::Public>>();

			let can_proceed = verify_signer_from_set(maybe_signers, &pub_key, &signature);

			if can_proceed.1 {
				if !dict.contains_key(&pub_key) {
					dict.insert(pub_key.clone(), Vec::new());
				}
				let temp = dict.get_mut(&pub_key).unwrap();
				temp.push(can_proceed.0.unwrap().encode());
			}
		}

		return dict
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
			CurrentAuthoritiesAccounts::<T>::put(&authorities_accounts);

			let next_id = Self::authority_set_id() + 1u64;

			<T::OnAuthoritySetChangeHandler as OnAuthoritySetChangeHandler<
				dkg_runtime_primitives::AuthoritySetId,
				T::AccountId,
			>>::on_authority_set_changed(next_id, authorities_accounts);

			<AuthoritySetId<T>>::put(next_id);

			let log: DigestItem = DigestItem::Consensus(
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
			Self::refresh_dkg_keys();
		}

		<NextAuthorities<T>>::put(&queued);
		NextAuthoritiesAccounts::<T>::put(&next_authorities_accounts);
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
		CurrentAuthoritiesAccounts::<T>::put(authority_account_ids);
		NextAuthoritiesAccounts::<T>::put(authority_account_ids);

		<T::OnAuthoritySetChangeHandler as OnAuthoritySetChangeHandler<
			dkg_runtime_primitives::AuthoritySetId,
			T::AccountId,
		>>::on_authority_set_changed(0, authority_account_ids.to_vec());
	}

	fn submit_genesis_public_key_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let mut agg_key_ref = StorageValueRef::persistent(AGGREGATED_PUBLIC_KEYS_AT_GENESIS);

		let mut submit_at_ref = StorageValueRef::persistent(SUBMIT_GENESIS_KEYS_AT);

		const RECENTLY_SENT: &str = "Already submitted a key in this session";

		let submit_at = submit_at_ref.get::<T::BlockNumber>();

		if let Ok(Some(submit_at)) = submit_at {
			if block_number < submit_at {
				frame_support::log::debug!(target: "dkg", "Offchain worker skipping public key submmission");
				return Ok(())
			} else {
				submit_at_ref.clear();
			}
		} else {
			Err(RECENTLY_SENT)?
		}

		if !Self::dkg_public_key().1.is_empty() {
			agg_key_ref.clear();
			return Ok(())
		}

		let agg_keys = agg_key_ref.get::<AggregatedPublicKeys>();

		let signer = Signer::<T, T::OffChainAuthId>::all_accounts();
		if !signer.can_sign() {
			Err("No local accounts available. Consider adding one via `author_insertKey` RPC.")?
		}

		if let Ok(Some(agg_keys)) = agg_keys {
			let _res = signer.send_signed_transaction(|_account| Call::submit_public_key {
				keys_and_signatures: agg_keys.clone(),
			});

			agg_key_ref.clear();
		}

		return Ok(())
	}

	fn submit_next_public_key_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let mut agg_key_ref = StorageValueRef::persistent(AGGREGATED_PUBLIC_KEYS);
		let mut submit_at_ref = StorageValueRef::persistent(SUBMIT_KEYS_AT);

		const RECENTLY_SENT: &str = "Already submitted a key in this session";

		let submit_at = submit_at_ref.get::<T::BlockNumber>();

		if let Ok(Some(submit_at)) = submit_at {
			if block_number < submit_at {
				frame_support::log::debug!(target: "dkg", "Offchain worker skipping public key submmission");
				return Ok(())
			} else {
				submit_at_ref.clear();
			}
		} else {
			Err(RECENTLY_SENT)?
		}

		if Self::next_dkg_public_key().is_some() {
			agg_key_ref.clear();
			return Ok(())
		}

		let agg_keys = agg_key_ref.get::<AggregatedPublicKeys>();

		let signer = Signer::<T, T::OffChainAuthId>::all_accounts();
		if !signer.can_sign() {
			Err("No local accounts available. Consider adding one via `author_insertKey` RPC.")?
		}

		if let Ok(Some(agg_keys)) = agg_keys {
			let _res = signer.send_signed_transaction(|_account| Call::submit_next_public_key {
				keys_and_signatures: agg_keys.clone(),
			});

			agg_key_ref.clear();
		}

		return Ok(())
	}

	fn submit_public_key_signature_onchain(
		block_number: T::BlockNumber,
	) -> Result<(), &'static str> {
		let mut pub_key_sig_ref = StorageValueRef::persistent(OFFCHAIN_PUBLIC_KEY_SIG);

		if Self::next_public_key_signature().is_some() {
			pub_key_sig_ref.clear();
			return Ok(())
		}

		let signature = pub_key_sig_ref.get::<dkg_runtime_primitives::crypto::Signature>();

		let signer = Signer::<T, T::OffChainAuthId>::all_accounts();
		if !signer.can_sign() {
			Err("No local accounts available. Consider adding one via `author_insertKey` RPC.")?
		}

		if let Ok(Some(signature)) = signature {
			let _ = signer.send_signed_transaction(|_account| Call::submit_public_key_signature {
				signature: signature.encode(),
			});
			frame_support::log::debug!(target: "dkg", "Offchain Submitting public key sig onchain {:?}", signature.encode());

			pub_key_sig_ref.clear();
		}

		return Ok(())
	}

	pub fn should_refresh(now: T::BlockNumber) -> bool {
		let (session_progress, ..) = <T::NextSessionRotation as EstimateNextSessionRotation<
			T::BlockNumber,
		>>::estimate_current_session_progress(now);
		if let Some(session_progress) = session_progress {
			let delay = RefreshDelay::<T>::get();
			let next_dkg_public_key_signature = Self::next_public_key_signature();
			return (delay <= session_progress) && next_dkg_public_key_signature.is_none()
		}
		false
	}

	pub fn verify_pub_key_signature(
		_origin: T::AccountId,
		signature: &Vec<u8>,
	) -> DispatchResultWithPostInfo {
		if let Some(pub_key) = Self::next_dkg_public_key() {
			dkg_runtime_primitives::utils::ensure_signed_by_dkg::<Self>(&signature, &pub_key.1)
				.map_err(|_| Error::<T>::InvalidSignature)?;

			if Self::next_public_key_signature().is_none() {
				NextPublicKeySignature::<T>::put((
					Self::authority_set_id() + 1u64,
					signature.clone(),
				));
				Self::deposit_event(Event::NextPublicKeySignatureSubmitted {
					pub_key_sig: signature.clone(),
				});
			}
		}

		Ok(().into())
	}

	pub fn refresh_dkg_keys() {
		let next_pub_key = Self::next_dkg_public_key();
		let next_pub_key_signature = Self::next_public_key_signature();
		let dkg_pub_key = Self::dkg_public_key();
		let pub_key_signature = Self::public_key_signature();
		NextDKGPublicKey::<T>::kill();
		NextPublicKeySignature::<T>::kill();
		if next_pub_key.is_some() && next_pub_key_signature.is_some() {
			DKGPublicKey::<T>::put(next_pub_key.clone().unwrap());
			DKGPublicKeySignature::<T>::put(next_pub_key_signature.clone().unwrap().1);
			PreviousPublicKey::<T>::put(dkg_pub_key.clone());
			UsedSignatures::<T>::mutate(|val| {
				val.push(pub_key_signature.clone());
			});

			let log: DigestItem = DigestItem::Consensus(
				DKG_ENGINE_ID,
				ConsensusLog::<T::DKGId>::KeyRefresh {
					new_key_signature: next_pub_key_signature.unwrap().1,
					old_public_key: dkg_pub_key.1,
					new_public_key: next_pub_key.unwrap().1,
				}
				.encode(),
			);
			<frame_system::Pallet<T>>::deposit_log(log);
		}
	}

	pub fn max_extrinsic_delay(_block_number: T::BlockNumber) -> T::BlockNumber {
		let refresh_delay = Self::refresh_delay();
		let session_length = <T::NextSessionRotation as EstimateNextSessionRotation<
			T::BlockNumber,
		>>::average_session_length();
		let max_delay = Permill::from_percent(50) * (refresh_delay * session_length);
		max_delay
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

	fn on_disabled(i: u32) {
		let log: DigestItem = DigestItem::Consensus(
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

impl<T: Config> GetDKGPublicKey for Pallet<T> {
	fn dkg_key() -> Vec<u8> {
		Self::dkg_public_key().1
	}
}
