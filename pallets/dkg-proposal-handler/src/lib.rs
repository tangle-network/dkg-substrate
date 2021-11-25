#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://substrate.dev/docs/en/knowledgebase/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use dkg_runtime_primitives::{
	EIP1559TransactionMessage, EIP2930TransactionMessage, LegacyTransactionMessage,
	OffchainSignedProposals, ProposalAction, ProposalHandlerTrait, ProposalNonce, ProposalType,
	TransactionV2, OFFCHAIN_SIGNED_PROPOSALS,
};
use frame_support::pallet_prelude::*;
use frame_system::{
	offchain::{AppCrypto, SendSignedTransaction, Signer},
	pallet_prelude::OriginFor,
};
use sp_runtime::offchain::storage::StorageValueRef;
use sp_std::{convert::TryFrom, vec::Vec};

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use dkg_runtime_primitives::ProposalType;
	use frame_support::dispatch::DispatchResultWithPostInfo;
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use sp_runtime::traits::AtLeast32Bit;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + CreateSignedTransaction<Call<Self>> {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// ChainID for anchor edges
		type ChainId: Encode + Decode + Parameter + AtLeast32Bit + Default + Copy;

		/// The identifier type for an offchain worker.
		type OffChainAuthorityId: AppCrypto<Self::Public, Self::Signature>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// /// All unsigned proposals.
	#[pallet::storage]
	#[pallet::getter(fn unsigned_proposals)]
	pub type UnsignedProposalQueue<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::ChainId,
		Blake2_128Concat,
		ProposalNonce,
		ProposalType,
	>;

	/// All signed proposals.
	#[pallet::storage]
	#[pallet::getter(fn signed_proposals)]
	pub type SignedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::ChainId,
		Blake2_128Concat,
		ProposalNonce,
		ProposalType,
	>;

	#[pallet::event]
	//#[pallet::metadata(T::AccountId = "AccountId")]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		ProposalAdded(T::AccountId, ProposalType),
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
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			let _res = Self::submit_signed_proposal_onchain(block_number);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn submit_signed_proposal(
			_origin: OriginFor<T>,
			prop: ProposalType,
		) -> DispatchResultWithPostInfo {
			let (data, signature) = match prop {
				ProposalType::EVMSigned { ref data, ref signature } => (data, signature),
				_ => return Err(Error::<T>::ProposalSignatureInvalid)?,
			};

			if let Ok(eth_transaction) = TransactionV2::decode(&mut &data[..]) {
				ensure!(
					Self::validate_ethereum_tx(&eth_transaction),
					Error::<T>::ProposalFormatInvalid
				);

				let (chain_id, nonce) = Self::extract_chain_id_and_nonce(&eth_transaction)?;

				ensure!(
					UnsignedProposalQueue::<T>::contains_key(chain_id, nonce),
					Error::<T>::ProposalDoesNotExist
				);
				ensure!(
					Self::validate_proposal_signature(&data, &signature),
					Error::<T>::ProposalSignatureInvalid
				);

				SignedProposals::<T>::insert(chain_id, nonce, prop.clone());

				UnsignedProposalQueue::<T>::remove(chain_id, nonce);
			}

			Ok(().into())
		}
	}
}

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	fn handle_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		if let Ok(eth_transaction) = TransactionV2::decode(&mut &proposal[..]) {
			ensure!(
				Self::validate_ethereum_tx(&eth_transaction),
				Error::<T>::ProposalFormatInvalid
			);

			let (chain_id, nonce) = Self::extract_chain_id_and_nonce(&eth_transaction)?;
			let unsigned_proposal = ProposalType::EVMUnsigned { data: proposal };
			UnsignedProposalQueue::<T>::insert(chain_id, nonce, unsigned_proposal);
		};

		return Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposals() -> Vec<(ProposalNonce, ProposalType)> {
		return UnsignedProposalQueue::<T>::iter()
			.map(|entry| (entry.1, entry.2.clone()))
			.collect()
	}

	// *** Offchain worker methods ***

	fn submit_signed_proposal_onchain(_block_number: T::BlockNumber) -> Result<(), &'static str> {
		let signer = Signer::<T, T::OffChainAuthorityId>::all_accounts();
		if !signer.can_sign() {
			return Err(
				"No local accounts available. Consider adding one via `author_insertKey` RPC.",
			)?
		}

		if let Ok(next_proposal) = Self::get_next_offchain_signed_proposal() {
			let _ = signer.send_signed_transaction(|_account| Call::submit_signed_proposal {
				prop: next_proposal.clone(),
			});
		}

		return Ok(())
	}

	fn get_next_offchain_signed_proposal() -> Result<ProposalType, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		if let Ok(Some(ser_props)) = proposals_ref.get::<Vec<u8>>() {
			let mut prop_wrapper = match OffchainSignedProposals::decode(&mut &ser_props[..]) {
				Ok(res) => res,
				Err(_) => return Err("Could not decode stored proposals")?,
			};

			if let Some(next_proposal) = prop_wrapper.proposals.pop_front() {
				let _update_res = proposals_ref.mutate(|val| match val {
					Ok(Some(_)) => Ok(prop_wrapper.encode()),
					_ => Err(()),
				});

				return Ok(next_proposal)
			}
		}

		return Err("No pending proposals found")?
	}

	// *** Validation methods ***

	fn validate_ethereum_tx(eth_transaction: &TransactionV2) -> bool {
		return match eth_transaction {
			TransactionV2::Legacy(_tx) => true,
			TransactionV2::EIP2930(_tx) => true,
			TransactionV2::EIP1559(_tx) => true,
		}
	}

	#[allow(dead_code)]
	fn validate_ethereum_tx_signature(eth_transaction: &TransactionV2) -> bool {
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
		sig[0..32].copy_from_slice(&sig_r[..]);
		sig[32..64].copy_from_slice(&sig_s[..]);
		sig[64] = sig_v;
		let mut msg = [0u8; 32];
		msg.copy_from_slice(&msg_hash[..]);

		return sp_io::crypto::secp256k1_ecdsa_recover(&sig, &msg).is_ok()
	}

	fn validate_proposal_signature(data: &Vec<u8>, signature: &Vec<u8>) -> bool {
		dkg_runtime_primitives::utils::validate_ecdsa_signature(data, signature)
	}

	// *** Utility methods ***

	fn extract_chain_id_and_nonce(
		eth_transaction: &TransactionV2,
	) -> core::result::Result<(T::ChainId, ProposalNonce), Error<T>> {
		let (chain_id, nonce) = match eth_transaction {
			TransactionV2::Legacy(tx) => {
				let chain_id: u64 = 0;
				let nonce = tx.nonce.as_u64();
				(chain_id, nonce)
			},
			TransactionV2::EIP2930(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u64();
				(chain_id, nonce)
			},
			TransactionV2::EIP1559(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u64();
				(chain_id, nonce)
			},
		};

		let chain_id = match T::ChainId::try_from(chain_id) {
			Ok(v) => v,
			Err(_) => return Err(Error::<T>::ChainIdInvalid)?,
		};

		return Ok((chain_id, nonce))
	}
}
