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
#![warn(missing_docs)]

//! A DKG+MMR pallet combo.
//!
//! While both DKG and Merkle Mountain Range (MMR) can be used separately,
//! these tools were designed to work together in unison.
//!
//! The pallet provides a standardized MMR Leaf format that is can be used
//! to bridge DKG+MMR-based networks (both standalone and polkadot-like).
//!
//! The MMR leaf contains:
//! 1. Block number and parent block hash.
//! 2. Merkle Tree Root Hash of next DKG validator set.
//! 3. Merkle Tree Root Hash of current parachain heads state.
//!
//! and thanks to versioning can be easily updated in the future.

use sp_runtime::traits::{Convert, Hash};
use sp_std::prelude::*;

use dkg_runtime_primitives::mmr::{DkgNextAuthoritySet, MmrLeaf, MmrLeafVersion};
use pallet_mmr::primitives::LeafDataProvider;

use codec::Encode;
use frame_support::traits::Get;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

/// A DKG consensus digest item with MMR root hash.
pub struct DepositDkgDigest<T>(sp_std::marker::PhantomData<T>);

impl<T> pallet_mmr::primitives::OnNewRoot<dkg_runtime_primitives::MmrRootHash>
	for DepositDkgDigest<T>
where
	T: pallet_mmr::Config<Hash = dkg_runtime_primitives::MmrRootHash>,
	T: pallet_dkg_metadata::Config,
{
	fn on_new_root(root: &<T as pallet_mmr::Config>::Hash) {
		let digest = sp_runtime::generic::DigestItem::Consensus(
			dkg_runtime_primitives::DKG_ENGINE_ID,
			codec::Encode::encode(&dkg_runtime_primitives::ConsensusLog::<
				<T as pallet_dkg_metadata::Config>::DKGId,
			>::MmrRoot(*root)),
		);
		<frame_system::Pallet<T>>::deposit_log(digest);
	}
}

/// Convert DKG secp256k1 public keys into Ethereum addresses
pub struct DkgEcdsaToEthereum;
impl Convert<dkg_runtime_primitives::crypto::AuthorityId, Vec<u8>> for DkgEcdsaToEthereum {
	fn convert(a: dkg_runtime_primitives::crypto::AuthorityId) -> Vec<u8> {
		use sp_core::crypto::Public;
		let compressed_key = a.as_slice();

		libsecp256k1::PublicKey::parse_slice(
			compressed_key,
			Some(libsecp256k1::PublicKeyFormat::Compressed),
		)
		// uncompress the key
		.map(|pub_key| pub_key.serialize().to_vec())
		// now convert to ETH address
		.map(|uncompressed| sp_io::hashing::keccak_256(&uncompressed[1..])[12..].to_vec())
		.map_err(|_| {
			log::error!(target: "runtime::dkg", "Invalid DKG PublicKey format!");
		})
		.unwrap_or_default()
	}
}

type MerkleRootOf<T> = <T as pallet_mmr::Config>::Hash;
type ParaId = u32;
type ParaHead = Vec<u8>;

/// A type that is able to return current list of parachain heads that end up in the MMR leaf.
pub trait ParachainHeadsProvider {
	/// Return a list of tuples containing a `ParaId` and Parachain Header data (ParaHead).
	///
	/// The returned data does not have to be sorted.
	fn parachain_heads() -> Vec<(ParaId, ParaHead)>;
}

/// A default implementation for runtimes without parachains.
impl ParachainHeadsProvider for () {
	fn parachain_heads() -> Vec<(ParaId, ParaHead)> {
		Default::default()
	}
}

#[frame_support::pallet]
pub mod pallet {
	#![allow(missing_docs)]

	use super::*;
	use frame_support::pallet_prelude::*;

	/// DKG-MMR pallet.
	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// The module's configuration trait.
	#[pallet::config]
	#[pallet::disable_frame_system_supertrait_check]
	pub trait Config: pallet_mmr::Config + pallet_dkg_metadata::Config {
		/// Current leaf version.
		///
		/// Specifies the version number added to every leaf that get's appended to the MMR.
		/// Read more in [`MmrLeafVersion`] docs about versioning leaves.
		type LeafVersion: Get<MmrLeafVersion>;

		/// Convert DKG AuthorityId to a form that would end up in the Merkle Tree.
		///
		/// For instance for ECDSA (secp256k1) we want to store uncompressed public keys (65 bytes)
		/// and later to Ethereum Addresses (160 bits) to simplify using them on Ethereum chain,
		/// but the rest of the Substrate codebase is storing them compressed (33 bytes) for
		/// efficiency reasons.
		type DkgAuthorityToMerkleLeaf: Convert<
			<Self as pallet_dkg_metadata::Config>::DKGId,
			Vec<u8>,
		>;

		/// Retrieve a list of current parachain heads.
		///
		/// The trait is implemented for `paras` module, but since not all chains might have parachains,
		/// and we want to keep the MMR leaf structure uniform, it's possible to use `()` as well to
		/// simply put dummy data to the leaf.
		type ParachainHeads: ParachainHeadsProvider;
	}

	/// Details of next DKG authority set.
	///
	/// This storage entry is used as cache for calls to [`update_dkg_next_authority_set`].
	#[pallet::storage]
	#[pallet::getter(fn dkg_next_authorities)]
	pub type DkgNextAuthorities<T: Config> =
		StorageValue<_, DkgNextAuthoritySet<MerkleRootOf<T>>, ValueQuery>;
}

impl<T: Config> LeafDataProvider for Pallet<T>
where
	MerkleRootOf<T>: From<pallet_dkg_merkle_tree::Hash> + Into<pallet_dkg_merkle_tree::Hash>,
{
	type LeafData = MmrLeaf<
		<T as frame_system::Config>::BlockNumber,
		<T as frame_system::Config>::Hash,
		MerkleRootOf<T>,
	>;

	fn leaf_data() -> Self::LeafData {
		MmrLeaf {
			version: T::LeafVersion::get(),
			parent_number_and_hash: frame_system::Pallet::<T>::leaf_data(),
			parachain_heads: Pallet::<T>::parachain_heads_merkle_root(),
			dkg_next_authority_set: Pallet::<T>::update_dkg_next_authority_set(),
		}
	}
}

impl<T: Config> pallet_dkg_merkle_tree::Hasher for Pallet<T>
where
	MerkleRootOf<T>: Into<pallet_dkg_merkle_tree::Hash>,
{
	fn hash(data: &[u8]) -> pallet_dkg_merkle_tree::Hash {
		<T as pallet_mmr::Config>::Hashing::hash(data).into()
	}
}

impl<T: Config> Pallet<T>
where
	MerkleRootOf<T>: From<pallet_dkg_merkle_tree::Hash> + Into<pallet_dkg_merkle_tree::Hash>,
{
	/// Returns latest root hash of a merkle tree constructed from all active parachain headers.
	///
	/// The leafs are sorted by `ParaId` to allow more efficient lookups and non-existence proofs.
	///
	/// NOTE this does not include parathreads - only parachains are part of the merkle tree.
	///
	/// NOTE This is an initial and inefficient implementation, which re-constructs
	/// the merkle tree every block. Instead we should update the merkle root in [Self::on_initialize]
	/// call of this pallet and update the merkle tree efficiently (use on-chain storage to persist inner nodes).
	fn parachain_heads_merkle_root() -> MerkleRootOf<T> {
		let mut para_heads = T::ParachainHeads::parachain_heads();
		para_heads.sort();
		let para_heads = para_heads.into_iter().map(|pair| pair.encode());
		pallet_dkg_merkle_tree::merkle_root::<Self, _, _>(para_heads).into()
	}

	/// Returns details of the next DKG authority set.
	///
	/// Details contain authority set id, authority set length and a merkle root,
	/// constructed from uncompressed secp256k1 public keys converted to Ethereum addresses
	/// of the next DKG authority set.
	///
	/// This function will use a storage-cached entry in case the set didn't change, or compute and cache
	/// new one in case it did.
	fn update_dkg_next_authority_set() -> DkgNextAuthoritySet<MerkleRootOf<T>> {
		let id = pallet_dkg_metadata::Pallet::<T>::authority_set_id() + 1;
		let current_next = Self::dkg_next_authorities();
		// avoid computing the merkle tree if validator set id didn't change.
		if id == current_next.id {
			return current_next
		}

		let dkg_addresses = pallet_dkg_metadata::Pallet::<T>::next_authorities()
			.into_iter()
			.map(T::DkgAuthorityToMerkleLeaf::convert)
			.collect::<Vec<_>>();
		let len = dkg_addresses.len() as u32;
		let root = pallet_dkg_merkle_tree::merkle_root::<Self, _, _>(dkg_addresses).into();
		let next_set = DkgNextAuthoritySet { id, len, root };
		// cache the result
		DkgNextAuthorities::<T>::put(&next_set);
		next_set
	}
}
