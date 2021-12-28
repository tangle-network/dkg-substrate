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
	Address, DKGPayloadKey, EIP1559TransactionMessage, EIP2930TransactionMessage,
	LegacyTransactionMessage, OffchainSignedProposals, ProposalAction, ProposalHandlerTrait,
	ProposalHeader, ProposalNonce, ProposalType, TransactionV2, OFFCHAIN_SIGNED_PROPOSALS,
};
use frame_support::pallet_prelude::*;
use frame_system::{
	offchain::{AppCrypto, SendSignedTransaction, Signer},
	pallet_prelude::OriginFor,
};
use sp_runtime::offchain::storage::StorageValueRef;
use sp_std::{convert::TryFrom, vec::Vec};

pub mod weights;
use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use dkg_runtime_primitives::{utils::ensure_signed_by_dkg, DKGPayloadKey, ProposalType};
	use frame_support::dispatch::DispatchResultWithPostInfo;
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use sp_runtime::traits::AtLeast32Bit;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + CreateSignedTransaction<Call<Self>> + pallet_dkg_metadata::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// ChainID for anchor edges
		type ChainId: Encode + Decode + Parameter + AtLeast32Bit + Default + Copy;
		/// The identifier type for an offchain worker.
		type OffChainAuthId: AppCrypto<Self::Public, Self::Signature>;
		/// Max number of signed proposal submissions per batch;
		#[pallet::constant]
		type MaxSubmissionsPerBatch: Get<u16>;
		/// Pallet weight information
		type WeightInfo: WeightInfo;
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
		/// Event When a Proposal Gets Signed by DKG.
		ProposalSigned {
			/// The Target EVM chain ID.
			chain_id: T::ChainId,
			/// The Payload Type or the Key.
			key: DKGPayloadKey,
			/// The Proposal Data.
			data: Vec<u8>,
			/// Signature of the hash of the proposal data.
			signature: Vec<u8>,
		},
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
		/// Proposal length exceeds max allowed per batch
		ProposalsLengthOverflow,
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
		#[pallet::weight(<T as Config>::WeightInfo::submit_signed_proposals(props.len() as u32))]
		#[frame_support::transactional]
		pub fn submit_signed_proposals(
			origin: OriginFor<T>,
			props: Vec<ProposalType>,
		) -> DispatchResultWithPostInfo {
			let sender = ensure_signed(origin)?;

			ensure!(
				props.len() <= T::MaxSubmissionsPerBatch::get() as usize,
				Error::<T>::ProposalsLengthOverflow
			);
			// log the caller, and the prop
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: props: {:?} by {:?}",
				&props,
				sender
			);

			for prop in &props {
				let (data, signature) = match prop {
					ProposalType::EVMSigned { data, signature } => (data, signature),
					ProposalType::AnchorUpdateSigned { data, signature } => (data, signature),
					ProposalType::TokenAddSigned { data, signature } => (data, signature),
					ProposalType::TokenRemoveSigned { data, signature } => (data, signature),
					ProposalType::WrappingFeeUpdateSigned { data, signature } => (data, signature),
					_ => Err(Error::<T>::ProposalSignatureInvalid)?,
				};

				ensure_signed_by_dkg::<pallet_dkg_metadata::Pallet<T>>(signature, data)
					.map_err(|_| Error::<T>::ProposalSignatureInvalid)?;

				// now we need to log the data and signature
				frame_support::log::debug!(
					target: "dkg_proposal_handler",
					"submit_signed_proposal: data: {:?}, signature: {:?}",
					data,
					signature
				);

				match prop {
					ProposalType::EVMSigned { .. } =>
						Self::handle_evm_signed_proposal(prop.clone())?,
					ProposalType::AnchorUpdateSigned { .. } =>
						Self::handle_anchor_update_signed_proposal(prop.clone())?,
					ProposalType::TokenAddSigned { .. } =>
						Self::handle_token_add_signed_proposal(prop.clone())?,
					ProposalType::TokenRemoveSigned { .. } =>
						Self::handle_token_remove_signed_proposal(prop.clone())?,
					ProposalType::WrappingFeeUpdateSigned { .. } =>
						Self::handle_wrapping_fee_update_signed_proposal(prop.clone())?,
					ProposalType::ResourceIdUpdateSigned { .. } =>
						Self::handle_resource_id_update_signed_proposal(prop.clone())?,
					_ => Err(Error::<T>::ProposalSignatureInvalid)?,
				}
			}

			Ok(().into())
		}

		/// Force submit an unsigned proposal to the DKG
		///
		/// There are certain proposals we'd like to be proposable only
		/// through root actions. The currently supported proposals are
		/// 	1. Updating
		#[pallet::weight(<T as Config>::WeightInfo::force_submit_unsigned_proposal())]
		pub fn force_submit_unsigned_proposal(
			origin: OriginFor<T>,
			prop: ProposalType,
		) -> DispatchResultWithPostInfo {
			// Call must come from root (likely from a democracy proposal passing)
			ensure_root(origin)?;
			// We ensure that only certain proposals are valid this way
			match prop {
				ProposalType::TokenAdd { ref data } => {
					let (chain_id, nonce) =
						Self::decode_token_add_proposal(&data).map(Into::into)?;
					UnsignedProposalQueue::<T>::insert(
						T::ChainId::from(chain_id),
						DKGPayloadKey::TokenAddProposal(nonce),
						prop.clone(),
					);
					Ok(().into())
				},
				ProposalType::TokenRemove { ref data } => {
					let (chain_id, nonce) =
						Self::decode_token_remove_proposal(&data).map(Into::into)?;
					UnsignedProposalQueue::<T>::insert(
						T::ChainId::from(chain_id),
						DKGPayloadKey::TokenRemoveProposal(nonce),
						prop.clone(),
					);
					Ok(().into())
				},
				ProposalType::WrappingFeeUpdate { ref data } => {
					let (chain_id, nonce) =
						Self::decode_wrapping_fee_update_proposal(&data).map(Into::into)?;
					UnsignedProposalQueue::<T>::insert(
						T::ChainId::from(chain_id),
						DKGPayloadKey::WrappingFeeUpdateProposal(nonce),
						prop.clone(),
					);
					Ok(().into())
				},
				ProposalType::ResourceIdUpdate { ref data } => {
					let (chain_id, nonce) =
						Self::decode_resource_id_update_proposal(&data).map(Into::into)?;
					UnsignedProposalQueue::<T>::insert(
						T::ChainId::from(chain_id),
						DKGPayloadKey::ResourceIdUpdateProposal(nonce),
						prop.clone(),
					);
					Ok(().into())
				},
				_ => Err(Error::<T>::ProposalFormatInvalid)?,
			}
		}
	}
}

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	fn handle_unsigned_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		if let Ok(eth_transaction) = TransactionV2::decode(&mut &proposal[..]) {
			ensure!(
				Self::validate_ethereum_tx(&eth_transaction),
				Error::<T>::ProposalFormatInvalid
			);

			let (chain_id, nonce) = Self::decode_evm_transaction(&eth_transaction)?;
			let unsigned_proposal = ProposalType::EVMUnsigned { data: proposal };
			UnsignedProposalQueue::<T>::insert(
				chain_id,
				DKGPayloadKey::EVMProposal(nonce),
				unsigned_proposal,
			);
		} else if let Ok((chain_id, nonce)) =
			Self::decode_anchor_update_proposal(&proposal).map(Into::into)
		{
			let unsigned_proposal = ProposalType::AnchorUpdate { data: proposal };
			UnsignedProposalQueue::<T>::insert(
				T::ChainId::from(chain_id),
				DKGPayloadKey::AnchorUpdateProposal(nonce),
				unsigned_proposal,
			);
		} else {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}

		return Ok(())
	}

	fn handle_evm_signed_proposal(prop: ProposalType) -> DispatchResult {
		let data = prop.data();
		let signature = prop.signature();
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

			let (chain_id, nonce) = Self::decode_evm_transaction(&eth_transaction)?;

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

			SignedProposals::<T>::insert(chain_id, DKGPayloadKey::EVMProposal(nonce), prop.clone());

			UnsignedProposalQueue::<T>::remove(chain_id, DKGPayloadKey::EVMProposal(nonce));
			// Emit event so frontend can react to it.
			Self::deposit_event(Event::<T>::ProposalSigned {
				chain_id,
				key: DKGPayloadKey::EVMProposal(nonce),
				data,
				signature,
			});
			Ok(().into())
		} else {
			Err(Error::<T>::ProposalFormatInvalid)?
		}
	}

	fn handle_anchor_update_signed_proposal(prop: ProposalType) -> DispatchResult {
		Self::handle_signed_proposal(prop, DKGPayloadKey::AnchorUpdateProposal(0u64))
	}

	fn handle_token_add_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult {
		Self::handle_signed_proposal(prop, DKGPayloadKey::TokenAddProposal(0u64))
	}

	fn handle_token_remove_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult {
		Self::handle_signed_proposal(prop, DKGPayloadKey::TokenRemoveProposal(0u64))
	}

	fn handle_wrapping_fee_update_signed_proposal(prop: ProposalType) -> DispatchResult {
		Self::handle_signed_proposal(prop, DKGPayloadKey::WrappingFeeUpdateProposal(0))
	}

	fn handle_resource_id_update_signed_proposal(
		prop: ProposalType,
	) -> frame_support::pallet_prelude::DispatchResult {
		Self::handle_signed_proposal(prop, DKGPayloadKey::ResourceIdUpdateProposal(0))
	}

	fn handle_signed_proposal(
		prop: ProposalType,
		payload_key_type: DKGPayloadKey,
	) -> DispatchResult {
		let data = prop.data();
		let signature = prop.signature();
		if let Ok((chain_id, nonce)) = Self::decode_proposal_header(&data).map(Into::into) {
			// log the chain id and nonce
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: chain_id: {:?}, nonce: {:?}",
				chain_id,
				nonce
			);

			let payload_key = match payload_key_type {
				DKGPayloadKey::AnchorUpdateProposal(_) =>
					DKGPayloadKey::AnchorUpdateProposal(nonce),
				DKGPayloadKey::TokenAddProposal(_) => DKGPayloadKey::TokenAddProposal(nonce),
				DKGPayloadKey::TokenRemoveProposal(_) => DKGPayloadKey::TokenRemoveProposal(nonce),
				DKGPayloadKey::WrappingFeeUpdateProposal(_) =>
					DKGPayloadKey::WrappingFeeUpdateProposal(nonce),
				DKGPayloadKey::ResourceIdUpdateProposal(_) =>
					DKGPayloadKey::ResourceIdUpdateProposal(nonce),
				_ => return Err(Error::<T>::ProposalFormatInvalid)?,
			};
			ensure!(
				UnsignedProposalQueue::<T>::contains_key(T::ChainId::from(chain_id), payload_key),
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

			SignedProposals::<T>::insert(T::ChainId::from(chain_id), payload_key, prop.clone());
			UnsignedProposalQueue::<T>::remove(T::ChainId::from(chain_id), payload_key);
			// Emit event so frontend can react to it.
			Self::deposit_event(Event::<T>::ProposalSigned {
				chain_id: T::ChainId::from(chain_id),
				key: payload_key,
				data,
				signature,
			});
			Ok(())
		} else {
			Err(Error::<T>::ProposalFormatInvalid)?
		}
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposals() -> Vec<(DKGPayloadKey, ProposalType)> {
		return UnsignedProposalQueue::<T>::iter()
			.map(|entry| (entry.1, entry.2.clone()))
			.collect()
	}

	pub fn is_existing_proposal(x: &ProposalType) -> bool {
		match x {
			ProposalType::EVMSigned { data, .. } => {
				if let Ok(eth_transaction) = TransactionV2::decode(&mut &data[..]) {
					if let Ok((chain_id, nonce)) = Self::decode_evm_transaction(&eth_transaction) {
						return !SignedProposals::<T>::contains_key(
							T::ChainId::from(chain_id),
							DKGPayloadKey::EVMProposal(nonce),
						)
					}
				}

				false
			},
			ProposalType::AnchorUpdateSigned { data, .. } => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(&data).map(Into::into) {
					return !SignedProposals::<T>::contains_key(
						T::ChainId::from(chain_id),
						DKGPayloadKey::AnchorUpdateProposal(nonce),
					)
				}
				false
			},
			ProposalType::TokenAddSigned { data, .. } => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(&data).map(Into::into) {
					return !SignedProposals::<T>::contains_key(
						T::ChainId::from(chain_id),
						DKGPayloadKey::TokenAddProposal(nonce),
					)
				}
				false
			},
			ProposalType::TokenRemoveSigned { data, .. } => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(data).map(Into::into) {
					return !SignedProposals::<T>::contains_key(
						T::ChainId::from(chain_id),
						DKGPayloadKey::TokenRemoveProposal(nonce),
					)
				}
				false
			},
			ProposalType::WrappingFeeUpdateSigned { data, .. } => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(data).map(Into::into) {
					return !SignedProposals::<T>::contains_key(
						T::ChainId::from(chain_id),
						DKGPayloadKey::WrappingFeeUpdateProposal(nonce),
					)
				}
				false
			},
			ProposalType::ResourceIdUpdateSigned { data, .. } => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(data).map(Into::into) {
					return !SignedProposals::<T>::contains_key(
						T::ChainId::from(chain_id),
						DKGPayloadKey::ResourceIdUpdateProposal(nonce),
					)
				}
				false
			},
			_ => false,
		}
	}

	// *** Offchain worker methods ***

	fn submit_signed_proposal_onchain(block_number: T::BlockNumber) -> Result<(), &'static str> {
		let signer = Signer::<T, <T as Config>::OffChainAuthId>::all_accounts();
		if !signer.can_sign() {
			return Err(
				"No local accounts available. Consider adding one via `author_insertKey` RPC.",
			)?
		}
		match Self::get_next_offchain_signed_proposal(block_number) {
			Ok(next_proposals) => {
				// We filter out all proposals that are already on chain
				let filtered_proposals = next_proposals
					.iter()
					.cloned()
					.filter(Self::is_existing_proposal)
					.collect::<Vec<_>>();

				// We split the vector into chunks of `T::MaxSubmissionsPerBatch` length and submit those chunks
				for chunk in filtered_proposals.chunks(T::MaxSubmissionsPerBatch::get() as usize) {
					let call = Call::<T>::submit_signed_proposals { props: chunk.to_vec() };
					let result = signer
						.send_signed_transaction(|_| call.clone())
						.into_iter()
						.map(|(_, r)| r)
						.collect::<Result<Vec<_>, _>>()
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

	fn get_next_offchain_signed_proposal(
		block_number: T::BlockNumber,
	) -> Result<Vec<ProposalType>, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut all_proposals = Vec::new();
		let res = proposals_ref.mutate::<OffchainSignedProposals<T::BlockNumber>, &'static str, _>(
			|res| {
				match res {
					Ok(Some(mut prop_wrapper)) => {
						// log the proposals
						frame_support::log::debug!(
							target: "dkg_proposal_handler",
							"Offchain signed proposals: {:?}",
							prop_wrapper.proposals
						);
						// log how many proposal batches are left
						frame_support::log::debug!(
							target: "dkg_proposal_handler",
							"Offchain signed proposals left: {}",
							prop_wrapper.proposals.len()
						);

						// We get all batches whose submission delay has been satisfied and remove them from offchain storage
						let mut to_remove = Vec::new();
						for (i, (props, submit_at)) in prop_wrapper.proposals.iter().enumerate() {
							if *submit_at <= block_number {
								all_proposals.extend_from_slice(&props[..]);
								to_remove.push(i);
							}
						}

						for i in to_remove {
							prop_wrapper.proposals.swap_remove(i);
						}

						Ok(prop_wrapper)
					},
					Ok(None) => Err("No signed proposals key stored"),
					Err(e) => {
						// log the error
						frame_support::log::warn!(
							target: "dkg_proposal_handler",
							"Failed to read offchain signed proposals: {:?}",
							e
						);
						Err("Error decoding offchain signed proposals")
					},
				}
			},
		);

		if res.is_err() || all_proposals.is_empty() {
			Err("Unable to get next proposal batch")?
		}

		return Ok(all_proposals)
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

	fn decode_evm_transaction(
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

	/// (resourceId: 32 Bytes, functionSig: 4 Bytes, nonce: 4 Bytes): at least 40 bytes
	fn decode_proposal_header(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		let header = ProposalHeader::decode(&mut &data[..])
			.map_err(|_| Error::<T>::ProposalFormatInvalid)?;
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Decoded Proposal Header: {:?} ({} bytes)",
			header,
			data.len(),
		);
		Ok(header)
	}

	/// (header: 40 Bytes, srcChainId: 4 Bytes, latestLeafIndex: 4 Bytes, merkleRoot: 32 Bytes) = 80 Bytes
	fn decode_anchor_update_proposal(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		if data.len() != 80 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		let header = Self::decode_proposal_header(data)?;
		let mut src_chain_id_bytes = [0u8; 4];
		src_chain_id_bytes.copy_from_slice(&data[40..44]);
		let src_chain_id = u32::from_le_bytes(src_chain_id_bytes);
		let mut latest_leaf_index_bytes = [0u8; 4];
		latest_leaf_index_bytes.copy_from_slice(&data[44..48]);
		let latest_leaf_index = u32::from_le_bytes(latest_leaf_index_bytes);
		let mut merkle_root_bytes = [0u8; 32];
		merkle_root_bytes.copy_from_slice(&data[48..80]);
		// 1. Should we check for the function signature?
		// 2. Should we check for the chainId != srcChainId?
		Ok(header)
	}

	/// (header: 40 Bytes, newFee: 1 Byte) = 41 Bytes
	fn decode_wrapping_fee_update_proposal(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		if data.len() != 41 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		let header = Self::decode_proposal_header(data)?;
		let new_fee = data.last().copied().expect("len is 41");
		// check if the fee is valid by checking if it is between 0 and 100
		if new_fee < 0 || new_fee > 100 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		Ok(header)
	}

	/// (header: 40 Bytes, newTokenAddress: 20 Bytes) = 60 Bytes
	fn decode_token_add_proposal(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		if data.len() != 60 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		let header = Self::decode_proposal_header(data)?;
		let mut new_token_address_bytes = [0u8; 20];
		new_token_address_bytes.copy_from_slice(&data[40..60]);
		let new_token_address = Address::from(new_token_address_bytes);
		Ok(header)
	}

	/// (header: 40 Bytes, removeTokenAddress: 20 Bytes) = 60 Bytes
	fn decode_token_remove_proposal(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		if data.len() != 60 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		let header = Self::decode_proposal_header(data)?;
		let mut token_address_bytes = [0u8; 20];
		token_address_bytes.copy_from_slice(&data[40..60]);
		let token_address = Address::from(token_address_bytes);
		Ok(header)
	}

	/// (header: 40 Bytes, newResourceId: 32, handlerAddress: 20, executionContextAddress: 20) = 112 Bytes
	fn decode_resource_id_update_proposal(data: &[u8]) -> Result<ProposalHeader, Error<T>> {
		if data.len() != 112 {
			return Err(Error::<T>::ProposalFormatInvalid)?
		}
		let header = Self::decode_proposal_header(data)?;
		let mut new_resource_id_bytes = [0u8; 32];
		new_resource_id_bytes.copy_from_slice(&data[40..72]);
		let mut handler_address_bytes = [0u8; 20];
		handler_address_bytes.copy_from_slice(&data[72..92]);
		let handler_address = Address::from(handler_address_bytes);
		let mut execution_context_address_bytes = [0u8; 20];
		execution_context_address_bytes.copy_from_slice(&data[92..112]);
		let execution_context_address = Address::from(execution_context_address_bytes);
		Ok(header)
	}

	#[cfg(feature = "runtime-benchmarks")]
	pub fn signed_proposals_len() -> usize {
		SignedProposals::<T>::iter_keys().collect::<Vec<_>>().len()
	}
}
