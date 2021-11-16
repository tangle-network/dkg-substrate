#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://substrate.dev/docs/en/knowledgebase/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use codec::{EncodeAppend, EncodeLike};
use frame_support::pallet_prelude::*;
use primitives::{
	EIP1559TransactionMessage, EIP2930TransactionMessage, LegacyTransactionMessage, ProposalAction,
	ProposalHandlerTrait, TransactionV2,
};

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, DispatchResultWithPostInfo, PostDispatchInfo},
		pallet_prelude::*,
	};
	use frame_system::pallet_prelude::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_dkg_proposals::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::event]
	//#[pallet::metadata(T::AccountId = "AccountId")]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		ProposalAdded(T::AccountId, T::Proposal),
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
		ProposalDoesNotExist,
		/// Chain id is invalid
		ChainIdInvalid,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn submit_signed_proposal(
			origin: OriginFor<T>,
			prop: T::Proposal,
		) -> DispatchResultWithPostInfo {
			let encoded_proposal = prop.encode();
			let eth_transaction = match TransactionV2::decode(&mut &encoded_proposal[..]) {
				Ok(tx) => tx,
				Err(_) => return Err(Error::<T>::ProposalFormatInvalid)?,
			};
			if let Err(err) = Self::validate_proposal_exists(&prop, &eth_transaction) {
				return Err(err)?
			}
			if let Err(err) = Self::validate_ethereum_tx_signature(&eth_transaction) {
				return Err(err)?
			}

			Ok(().into())
		}
	}
}

impl<T: Config> ProposalHandlerTrait<T::Proposal> for Pallet<T> {
	fn handle_proposal(proposal: T::Proposal, action: ProposalAction) -> DispatchResult {
		let encoded_proposal = proposal.encode();

		if let Ok(eth_transaction) = TransactionV2::decode(&mut &encoded_proposal[..]) {
			return Self::handle_ethereum_tx(&eth_transaction, action)
		};

		return Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** Handler methods ***

	fn handle_ethereum_tx(
		eth_transaction: &TransactionV2,
		action: ProposalAction,
	) -> DispatchResult {
		if let Err(err) = Self::validate_ethereum_tx(eth_transaction) {
			return Err(err)
		}

		// TODO: handle validated tx
		Ok(())
	}

	// *** Validation methods ***

	fn validate_ethereum_tx(eth_transaction: &TransactionV2) -> DispatchResult {
		return match eth_transaction {
			TransactionV2::Legacy(tx) => Ok(()),
			TransactionV2::EIP2930(tx) => Ok(()),
			TransactionV2::EIP1559(tx) => Ok(()),
		}
	}

	fn validate_proposal_exists(
		prop: &T::Proposal,
		decoded_prop: &TransactionV2,
	) -> DispatchResult {
		let (chain_id, nonce, msg_hash) = match decoded_prop {
			TransactionV2::Legacy(tx) => {
				let chain_id: u64 = 0;
				let nonce = tx.nonce.as_u64();
				let hash = LegacyTransactionMessage::from(tx.clone()).hash();
				(chain_id, nonce, hash)
			},
			TransactionV2::EIP2930(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u64();
				let hash = EIP2930TransactionMessage::from(tx.clone()).hash();
				(chain_id, nonce, hash)
			},
			TransactionV2::EIP1559(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u64();
				let hash = EIP1559TransactionMessage::from(tx.clone()).hash();
				(chain_id, nonce, hash)
			},
		};

		if !pallet_dkg_proposals::Pallet::<T>::proposal_exists(chain_id, nonce, prop.clone()) {
			return Err(Error::<T>::ProposalDoesNotExist)?
		};

		Ok(())
	}

	fn validate_ethereum_tx_signature(eth_transaction: &TransactionV2) -> DispatchResult {
		let (sig_r, sig_s, sig_v, msg_hash) = match eth_transaction {
			TransactionV2::Legacy(tx) => {
				let r = tx.signature.r().clone();
				let s = tx.signature.s().clone();
				let v = tx.signature.standard_v();
				let hash = LegacyTransactionMessage::from(tx.clone()).hash();
				(r, s, v, hash)
			},
			TransactionV2::EIP2930(tx) => {
				let r = tx.r.clone();
				let s = tx.s.clone();
				let v = if tx.odd_y_parity { 1 } else { 0 };
				let hash = EIP2930TransactionMessage::from(tx.clone()).hash();
				(r, s, v, hash)
			},
			TransactionV2::EIP1559(tx) => {
				let r = tx.r.clone();
				let s = tx.s.clone();
				let v = if tx.odd_y_parity { 1 } else { 0 };
				let hash = EIP1559TransactionMessage::from(tx.clone()).hash();
				(r, s, v, hash)
			},
		};

		let mut sig = [0u8; 65];
		let mut msg = [0u8; 32];
		sig[0..32].copy_from_slice(&sig_r[..]);
		sig[32..64].copy_from_slice(&sig_s[..]);
		sig[64] = sig_v;
		msg.copy_from_slice(&msg_hash[..]);

		return match sp_io::crypto::secp256k1_ecdsa_recover(&sig, &msg) {
			Ok(_) => Ok(()),
			Err(_) => Err(Error::<T>::ProposalSignatureInvalid)?,
		}
	}
}
