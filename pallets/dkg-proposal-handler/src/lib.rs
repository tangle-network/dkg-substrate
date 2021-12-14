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
	DKGPayloadKey, EIP1559TransactionMessage, EIP2930TransactionMessage, LegacyTransactionMessage,
	OffchainSignedProposals, ProposalAction, ProposalHandlerTrait, ProposalNonce, ProposalType,
	TransactionV2, OFFCHAIN_SIGNED_PROPOSALS, U256,
};
use frame_support::pallet_prelude::*;
use frame_system::{
	offchain::{AppCrypto, SubmitTransaction},
	pallet_prelude::OriginFor,
};
use sp_runtime::offchain::storage::StorageValueRef;
use sp_std::{convert::TryFrom, vec::Vec};

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use dkg_runtime_primitives::{DKGPayloadKey, ProposalType};
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
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;
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
		DKGPayloadKey,
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
		DKGPayloadKey,
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
		ProposalDoesNotExists,
		/// Proposal with the ID has already been submitted
		ProposalAlreadyExists,
		/// Chain id is invalid
		ChainIdInvalid,
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
		#[pallet::weight(0)]
		pub fn submit_signed_proposal(
			_origin: OriginFor<T>,
			prop: ProposalType,
		) -> DispatchResultWithPostInfo {
			// let sender = ensure_signed(origin)?;
			// log the caller, and the prop
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: prop: {:?}",
				prop
			);
			let (data, signature) = match prop {
				ProposalType::EVMSigned { ref data, ref signature } => (data, signature),
				ProposalType::AnchorUpdateSigned { ref data, ref signature } => (data, signature),
				_ => return Err(Error::<T>::ProposalSignatureInvalid)?,
			};
			// now we need to log the data and signature
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: data: {:?}, signature: {:?}",
				data,
				signature
			);
			if let Ok(eth_transaction) = TransactionV2::decode(&mut &data[..]) {
				// log that we are decoding the transaction as TransactionV2
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"submit_signed_proposal: decoding as TransactionV2"
				);
				ensure!(
					Self::validate_ethereum_tx(&eth_transaction),
					Error::<T>::ProposalFormatInvalid
				);

				let (chain_id, nonce) = Self::extract_chain_id_and_nonce(&eth_transaction)?;

				ensure!(
					UnsignedProposalQueue::<T>::contains_key(
						chain_id,
						DKGPayloadKey::EVMProposal(nonce)
					),
					Error::<T>::ProposalDoesNotExists
				);
				ensure!(
					Self::validate_proposal_signature(&data, &signature),
					Error::<T>::ProposalSignatureInvalid
				);

				SignedProposals::<T>::insert(
					chain_id,
					DKGPayloadKey::EVMProposal(nonce),
					prop.clone(),
				);

				UnsignedProposalQueue::<T>::remove(chain_id, DKGPayloadKey::EVMProposal(nonce));
				Ok(().into())
			} else if let Ok((chain_id, nonce)) = Self::decode_anchor_update(&data) {
				// log the chain id and nonce
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"submit_signed_proposal: chain_id: {:?}, nonce: {:?}",
					chain_id,
					nonce
				);
				ensure!(
					UnsignedProposalQueue::<T>::contains_key(
						chain_id,
						DKGPayloadKey::AnchorUpdateProposal(nonce)
					),
					Error::<T>::ProposalDoesNotExists
				);
				// log that proposal exist in the unsigned queue
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"submit_signed_proposal: proposal exist in the unsigned queue"
				);
				ensure!(
					Self::validate_proposal_signature(&data, &signature),
					Error::<T>::ProposalSignatureInvalid
				);

				// log that the signature is valid
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"submit_signed_proposal: signature is valid"
				);

				SignedProposals::<T>::insert(
					chain_id,
					DKGPayloadKey::AnchorUpdateProposal(nonce),
					prop.clone(),
				);
				UnsignedProposalQueue::<T>::remove(
					chain_id,
					DKGPayloadKey::AnchorUpdateProposal(nonce),
				);
				Ok(().into())
			} else {
				Err(Error::<T>::ProposalFormatInvalid)?
			}
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
			UnsignedProposalQueue::<T>::insert(
				chain_id,
				DKGPayloadKey::EVMProposal(nonce),
				unsigned_proposal,
			);
		} else if let Ok((chain_id, nonce)) = Self::decode_anchor_update(&proposal) {
			let unsigned_proposal = ProposalType::AnchorUpdate { data: proposal };
			UnsignedProposalQueue::<T>::insert(
				chain_id,
				DKGPayloadKey::AnchorUpdateProposal(nonce),
				unsigned_proposal,
			);
		}

		return Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposals() -> Vec<(DKGPayloadKey, ProposalType)> {
		return UnsignedProposalQueue::<T>::iter()
			.map(|entry| (entry.1, entry.2.clone()))
			.collect()
	}

	// *** Offchain worker methods ***

	fn submit_signed_proposal_onchain(_block_number: T::BlockNumber) -> Result<(), &'static str> {
		match Self::get_next_offchain_signed_proposal() {
			Ok(next_proposal) => {
				// send unsigned transaction to the chain
				let call = Call::<T>::submit_signed_proposal { prop: next_proposal.clone() };
				let result = SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
					call.clone().into(),
				)
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

	fn get_next_offchain_signed_proposal() -> Result<ProposalType, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		match proposals_ref.get::<OffchainSignedProposals>() {
			Ok(Some(mut prop_wrapper)) => {
				// log the proposals
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"Offchain signed proposals: {:?}",
					prop_wrapper.proposals
				);
				// log how many proposals are left
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"Offchain signed proposals left: {}",
					prop_wrapper.proposals.len()
				);
				match prop_wrapper.proposals.pop_front() {
					Some(next_proposal) => {
						proposals_ref.set(&prop_wrapper);
						return Ok(next_proposal)
					},
					None => {
						// log that there are no more proposals
						frame_support::log::debug!(
							target: "dkg_proposal_handler",
							"No more offchain signed proposals"
						);
						return Err("No more offchain signed proposals")
					},
				}
			},
			Ok(None) => return Err("No signed proposals key stored")?,
			Err(e) => {
				// log the error
				frame_support::log::warn!(
					target: "dkg_proposal_handler",
					"Failed to read offchain signed proposals: {:?}",
					e
				);
			},
		};

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

	fn decode_anchor_update(data: &[u8]) -> Result<(T::ChainId, ProposalNonce), Error<T>> {
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Decoding anchor update: {:?} ({} bytes)",
			data,
			data.len(),
		);
		// check if the data length is 118 bytes [
		// 	anchor_handler_address_0x_prefixed(22),
		// 	chain_id(32),
		// 	nonce(32),
		// 	merkle_root(32)
		// ]
		if data.len() != 22 + 32 + 32 + 32 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		// skip the first 22 bytes and then read the next 32 bytes as the chain id as U256
		let chain_id = U256::from(&data[22..(22 + 32)]).as_u64();
		let chain_id = match T::ChainId::try_from(chain_id) {
			Ok(v) => v,
			Err(_) => return Err(Error::<T>::ChainIdInvalid)?,
		};
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Got Chain id: {:?}",
			chain_id,
		);
		// read the next 32 bytes as the nonce
		let nonce = U256::from(&data[(22 + 32)..64]).as_u64();
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Got Nonce: {}",
			nonce,
		);
		// now return the result
		Ok((chain_id, nonce))
	}
}
