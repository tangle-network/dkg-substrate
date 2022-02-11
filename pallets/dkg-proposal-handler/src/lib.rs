#![cfg_attr(not(feature = "std"), no_std)]

use dkg_runtime_primitives::traits::OnDKGPublicKeyChangeHandler;
/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://substrate.dev/docs/en/knowledgebase/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use dkg_runtime_primitives::{
	Address,
	ChainIdType,
	DKGPayloadKey, EIP1559TransactionMessage, EIP2930TransactionMessage, LegacyTransactionMessage,
	OffchainSignedProposals, Proposal, ProposalAction, ProposalHandlerTrait, ProposalHeader,
	ProposalKind, ProposalNonce, TransactionV2, OFFCHAIN_SIGNED_PROPOSALS,
};
use frame_support::pallet_prelude::*;
use frame_system::{
	offchain::{AppCrypto, SendSignedTransaction, Signer},
	pallet_prelude::OriginFor,
};
use sp_runtime::{offchain::storage::StorageValueRef, traits::Zero};
use sp_std::{convert::TryFrom, vec::Vec};

pub mod weights;
use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use dkg_runtime_primitives::{
		utils::ensure_signed_by_dkg, DKGPayloadKey, Proposal, ProposalKind,
	};
	use frame_support::dispatch::DispatchResultWithPostInfo;
	use frame_system::{offchain::CreateSignedTransaction, pallet_prelude::*};
	use sp_runtime::traits::AtLeast32BitUnsigned;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + CreateSignedTransaction<Call<Self>> + pallet_dkg_metadata::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// ChainID for anchor edges
		type ChainId: Encode + Decode + Parameter + AtLeast32BitUnsigned + Default + Copy;
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

	/// All unsigned proposals.
	#[pallet::storage]
	#[pallet::getter(fn unsigned_proposals)]
	pub type UnsignedProposalQueue<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		ChainIdType<T::ChainId>,
		Blake2_128Concat,
		DKGPayloadKey,
		Proposal,
	>;

	/// All signed proposals.
	#[pallet::storage]
	#[pallet::getter(fn signed_proposals)]
	pub type SignedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		ChainIdType<T::ChainId>,
		Blake2_128Concat,
		DKGPayloadKey,
		Proposal,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		ProposalAdded(T::AccountId, Proposal),
		/// Event Emitted when we encounter a Proposal with invalid Signature.
		InvalidProposalSignature {
			/// The Type of the Proposal.
			kind: ProposalKind,
			/// Proposal Payload.
			data: Vec<u8>,
			/// The Invalid Signature.
			invalid_signature: Vec<u8>,
		},
		/// Event When a Proposal Gets Signed by DKG.
		ProposalSigned {
			/// The Target EVM chain ID.
			chain_id: ChainIdType<T::ChainId>,
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
			props: Vec<Proposal>,
		) -> DispatchResultWithPostInfo {
			let sender = ensure_signed(origin)?;

			ensure!(
				props.len() <= T::MaxSubmissionsPerBatch::get() as usize,
				Error::<T>::ProposalsLengthOverflow
			);

			// log the caller, and the props.
			frame_support::log::debug!(
				target: "dkg_proposal_handler",
				"submit_signed_proposal: props: {:?} by {:?}",
				&props,
				sender
			);

			for prop in &props {
				if let Proposal::Signed { kind, data, signature } = prop {
					let result =
						ensure_signed_by_dkg::<pallet_dkg_metadata::Pallet<T>>(signature, data)
							.map_err(|_| Error::<T>::ProposalSignatureInvalid);
					match result {
						Ok(_) => {
							// Do nothing, it is all good.
						},
						Err(e) => {
							// this is a bad signature.
							// we emit it as an event.
							Self::deposit_event(Event::InvalidProposalSignature {
								kind: kind.clone(),
								data: data.clone(),
								invalid_signature: signature.clone(),
							});
							frame_support::log::error!(
								target: "dkg_proposal_handler",
								"Invalid proposal signature with kind: {:?}, data: {:?}, sig: {:?}",
								kind,
								data,
								signature
							);
							// skip it.
							continue
						},
					}

					// now we need to log the data and signature
					frame_support::log::debug!(
						target: "dkg_proposal_handler",
						"submit_signed_proposal: data: {:?}, signature: {:?}",
						data,
						signature
					);

					let prop = prop.clone();

					Self::handle_signed_proposal(prop)?;

					continue
				}

				Err(Error::<T>::ProposalSignatureInvalid)?;
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
			prop: Proposal,
		) -> DispatchResultWithPostInfo {
			// Call must come from root (likely from a democracy proposal passing)
			ensure_root(origin)?;

			// We ensure that only certain proposals are valid this way
			if let Proposal::Unsigned { kind, data } = &prop {
				let chain_key = match kind {
					ProposalKind::EVM => {
						let eth_transaction = TransactionV2::decode(&mut &data[..])
							.map_err(|_| Error::<T>::ProposalFormatInvalid)?;

						ensure!(
							Self::validate_ethereum_tx(&eth_transaction),
							Error::<T>::ProposalFormatInvalid
						);

						let (chain_id, nonce) = Self::decode_evm_transaction(&eth_transaction)?;
						Some((chain_id, DKGPayloadKey::EVMProposal(nonce)))
					},
					ProposalKind::TokenAdd => {
						let (chain_id, nonce) =
							Self::decode_token_add_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::TokenAddProposal(nonce)))
					},
					ProposalKind::TokenRemove => {
						let (chain_id, nonce) =
							Self::decode_token_remove_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::TokenRemoveProposal(nonce)))
					},
					ProposalKind::WrappingFeeUpdate => {
						let (chain_id, nonce) =
							Self::decode_wrapping_fee_update_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::WrappingFeeUpdateProposal(nonce)))
					},
					ProposalKind::ResourceIdUpdate => {
						let (chain_id, nonce) =
							Self::decode_resource_id_update_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::ResourceIdUpdateProposal(nonce)))
					},
					ProposalKind::RescueTokens => {
						let (chain_id, nonce) =
							Self::decode_rescue_tokens_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::RescueTokensProposal(nonce)))
					},
					ProposalKind::MaxDepositLimitUpdate => {
						let (chain_id, nonce) =
							Self::decode_configurable_limit_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::MaxDepositLimitUpdateProposal(nonce)))
					},
					ProposalKind::MinWithdrawalLimitUpdate => {
						let (chain_id, nonce) =
							Self::decode_configurable_limit_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::MinWithdrawLimitUpdateProposal(nonce)))
					},
					ProposalKind::MaxExtLimitUpdate => {
						let (chain_id, nonce) =
							Self::decode_configurable_limit_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::MaxExtLimitUpdateProposal(nonce)))
					},
					ProposalKind::MaxFeeLimitUpdate => {
						let (chain_id, nonce) =
							Self::decode_configurable_limit_proposal(data).map(Into::into)?;
						Some((chain_id, DKGPayloadKey::MaxFeeLimitUpdateProposal(nonce)))
					},
					_ => None,
				};

				if let Some((chain_id, key)) = chain_key {
					UnsignedProposalQueue::<T>::insert(chain_id, key, prop.clone());
					return Ok(().into())
				}
			}

			Err(Error::<T>::ProposalFormatInvalid)?
		}
	}
}

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	fn handle_unsigned_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		if let Ok((chain_id, nonce)) =
			Self::decode_anchor_update_proposal(&proposal).map(Into::into)
		{
			let unsigned_proposal =
				Proposal::Unsigned { data: proposal, kind: ProposalKind::AnchorUpdate };

			UnsignedProposalQueue::<T>::insert(
				chain_id,
				DKGPayloadKey::AnchorUpdateProposal(nonce),
				unsigned_proposal,
			);

			return Ok(())
		}

		Err(Error::<T>::ProposalFormatInvalid)?
	}

	fn handle_unsigned_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		let unsigned_proposal =
			Proposal::Unsigned { data: proposal.encode(), kind: ProposalKind::Refresh };

		UnsignedProposalQueue::<T>::insert(
			ChainIdType::<T::ChainId>::EVM(T::ChainId::zero()),
			DKGPayloadKey::RefreshVote(proposal.nonce),
			unsigned_proposal,
		);

		Ok(().into())
	}

	fn handle_signed_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		UnsignedProposalQueue::<T>::remove(
			ChainIdType::<T::ChainId>::EVM(T::ChainId::zero()),
			DKGPayloadKey::RefreshVote(proposal.nonce),
		);

		Ok(().into())
	}

	fn handle_signed_proposal(prop: Proposal) -> DispatchResult {
		let data = prop.data();
		let signature = prop.signature();

		let (chain_id, nonce) = match prop.kind() {
			ProposalKind::EVM => {
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
					(chain_id, nonce)
				} else {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
			},
			_ => {
				if let Ok((chain_id, nonce)) = Self::decode_proposal_header(&data).map(Into::into) {
					(chain_id, nonce)
				} else {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
			}
		};
		// log the chain id and nonce
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"submit_signed_proposal: chain_id: {:?}, nonce: {:?}",
			chain_id,
			nonce
		);

		let payload_key = match prop.kind() {
			EVM => DKGPayloadKey::EVMProposal(nonce),
			AnchorUpdate => DKGPayloadKey::AnchorUpdateProposal(nonce),
			TokenAdd => DKGPayloadKey::TokenAddProposal(nonce),
			TokenRemove => DKGPayloadKey::TokenRemoveProposal(nonce),
			WrappingFeeUpdate => DKGPayloadKey::WrappingFeeUpdateProposal(nonce),
			ResourceIdUpdate => DKGPayloadKey::ResourceIdUpdateProposal(nonce),
			RescueTokens => DKGPayloadKey::RescueTokensProposal(nonce),
			MaxDepositLimitUpdate => DKGPayloadKey::MaxDepositLimitUpdateProposal(nonce),
			MinWithdrawalLimitUpdate => DKGPayloadKey::MinWithdrawLimitUpdateProposal(nonce),
			MaxExtLimitUpdate => DKGPayloadKey::MaxExtLimitUpdateProposal(nonce),
			MaxFeeLimitUpdate => DKGPayloadKey::MaxFeeLimitUpdateProposal(nonce),
			_ => return Err(Error::<T>::ProposalFormatInvalid)?,
		};

		ensure!(
			UnsignedProposalQueue::<T>::contains_key(chain_id.clone(), payload_key),
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

		SignedProposals::<T>::insert(chain_id.clone(), payload_key, prop.clone());
		UnsignedProposalQueue::<T>::remove(chain_id.clone(), payload_key);
		// Emit event so frontend can react to it.
		Self::deposit_event(Event::<T>::ProposalSigned {
			chain_id,
			key: payload_key,
			data: data.clone(),
			signature,
		});

		Ok(())
	}
}

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposals() -> Vec<((ChainIdType<T::ChainId>, DKGPayloadKey), Proposal)> {
		return UnsignedProposalQueue::<T>::iter()
			.map(|entry| ((entry.0, entry.1), entry.2.clone()))
			.collect()
	}

	pub fn is_existing_proposal(prop: &Proposal) -> bool {
		if let Proposal::Signed { kind, ref data, .. } = prop {
			let mut found = None;

			match kind {
				ProposalKind::EVM =>
					if let Ok(eth_transaction) = TransactionV2::decode(&mut &data[..]) {
						if let Ok((chain_id, nonce)) =
							Self::decode_evm_transaction(&eth_transaction)
						{
							found = Some((chain_id, DKGPayloadKey::EVMProposal(nonce)));
						}
					},
				ProposalKind::AnchorUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(&data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::AnchorUpdateProposal(nonce)));
					}
				},
				ProposalKind::TokenAdd => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(&data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::TokenAddProposal(nonce)));
					}
				},
				ProposalKind::TokenRemove => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::TokenRemoveProposal(nonce)));
					}
				},
				ProposalKind::WrappingFeeUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::WrappingFeeUpdateProposal(nonce)));
					}
				},
				ProposalKind::ResourceIdUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::ResourceIdUpdateProposal(nonce)));
					}
				},
				ProposalKind::RescueTokens => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::RescueTokensProposal(nonce)));
					}
				},
				ProposalKind::MaxDepositLimitUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found =
							Some((chain_id, DKGPayloadKey::MaxDepositLimitUpdateProposal(nonce)));
					}
				},
				ProposalKind::MinWithdrawalLimitUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found =
							Some((chain_id, DKGPayloadKey::MinWithdrawLimitUpdateProposal(nonce)));
					}
				},
				ProposalKind::MaxExtLimitUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::MaxExtLimitUpdateProposal(nonce)));
					}
				},
				ProposalKind::MaxFeeLimitUpdate => {
					if let Ok((chain_id, nonce)) =
						Self::decode_proposal_header(data).map(Into::into)
					{
						found = Some((chain_id, DKGPayloadKey::MaxFeeLimitUpdateProposal(nonce)));
					}
				},
				_ => (),
			};

			if let Some((chain_id, key)) = found {
				return !SignedProposals::<T>::contains_key(chain_id, key)
			}
		}

		false
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

				// We split the vector into chunks of `T::MaxSubmissionsPerBatch` length and submit
				// those chunks
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
	) -> Result<Vec<Proposal>, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut all_proposals = Vec::new();
		let res = proposals_ref.mutate::<OffchainSignedProposals<T::BlockNumber>, _, _>(|res| {
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

					// We get all batches whose submission delay has been satisfied
					all_proposals = prop_wrapper
						.proposals
						.iter()
						.filter_map(
							|(props, submit_at)| {
								if *submit_at <= block_number {
									Some(props)
								} else {
									None
								}
							},
						)
						.cloned()
						.flatten()
						.collect::<Vec<_>>();
					// then we need to keep only the batches that are not yet submitted
					prop_wrapper.proposals.retain(|(_, submit_at)| *submit_at > block_number);
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
		});

		if res.is_err() || all_proposals.is_empty() {
			return Err("Unable to get next proposal batch")
		}

		Ok(all_proposals)
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
	) -> core::result::Result<(ChainIdType<T::ChainId>, ProposalNonce), Error<T>> {
		let (chain_id, nonce) = match eth_transaction {
			TransactionV2::Legacy(tx) => {
				let chain_id: u64 = 0;
				let nonce = tx.nonce.as_u32();
				(chain_id, nonce)
			},
			TransactionV2::EIP2930(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u32();
				(chain_id, nonce)
			},
			TransactionV2::EIP1559(tx) => {
				let chain_id: u64 = tx.chain_id;
				let nonce = tx.nonce.as_u32();
				(chain_id, nonce)
			},
		};

		let chain_id = match T::ChainId::try_from(chain_id) {
			Ok(v) => v,
			Err(_) => return Err(Error::<T>::ChainIdInvalid)?,
		};

		return Ok((ChainIdType::EVM(chain_id), nonce))
	}

	/// (resourceId: 32 Bytes, functionSig: 4 Bytes, nonce: 4 Bytes): at least 40 bytes
	fn decode_proposal_header(data: &[u8]) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = ProposalHeader::<T::ChainId>::decode(&mut &data[..])
			.map_err(|_| Error::<T>::ProposalFormatInvalid)?;
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Decoded Proposal Header: {:?} ({} bytes)",
			header,
			data.len(),
		);
		Ok(header)
	}

	/// (header: 40 Bytes, srcChainId: 4 Bytes, latestLeafIndex: 4 Bytes, merkleRoot: 32 Bytes) = 80
	/// Bytes
	fn decode_anchor_update_proposal(data: &[u8]) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		frame_support::log::debug!(
			target: "dkg_proposal_handler",
			"🕸️ Decoded Anchor Update Proposal: {:?} ({} bytes)",
			data,
			data.len(),
		);

		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 80 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut src_chain_id_bytes = [0u8; 4];
				src_chain_id_bytes.copy_from_slice(&data[40..44]);
				let src_chain_id = u32::from_be_bytes(src_chain_id_bytes);
				let mut latest_leaf_index_bytes = [0u8; 4];
				latest_leaf_index_bytes.copy_from_slice(&data[44..48]);
				let latest_leaf_index = u32::from_be_bytes(latest_leaf_index_bytes);
				let mut merkle_root_bytes = [0u8; 32];
				merkle_root_bytes.copy_from_slice(&data[48..80]);
				// 1. Should we check for the function signature?
				// 2. Should we check for the chainId != srcChainId?
				// TODO: do something with them here.
				let _ = src_chain_id;
				let _ = latest_leaf_index;
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, newFee: 1 Byte) = 41 Bytes
	fn decode_wrapping_fee_update_proposal(
		data: &[u8],
	) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 41 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let new_fee = data.last().copied().expect("len is 41");
				// check if the fee is valid by checking if it is between 0 and 100
				// note that u8 is unsigned, so we need to check for 0x00 and 0xFF
				if new_fee > 100 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, newTokenAddress: 20 Bytes) = 60 Bytes
	fn decode_token_add_proposal(data: &[u8]) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 60 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut new_token_address_bytes = [0u8; 20];
				new_token_address_bytes.copy_from_slice(&data[40..60]);
				let new_token_address = Address::from(new_token_address_bytes);
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, tokenAddress: 20 Bytes) = 60 Bytes
	fn decode_token_remove_proposal(data: &[u8]) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 60 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut token_address_bytes = [0u8; 20];
				token_address_bytes.copy_from_slice(&data[40..60]);
				let token_address = Address::from(token_address_bytes);
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, newResourceId: 32, handlerAddress: 20, executionContextAddress: 20) = 112
	/// Bytes
	fn decode_resource_id_update_proposal(
		data: &[u8],
	) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 112 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut new_resource_id_bytes = [0u8; 32];
				new_resource_id_bytes.copy_from_slice(&data[40..72]);
				let mut handler_address_bytes = [0u8; 20];
				handler_address_bytes.copy_from_slice(&data[72..92]);
				let handler_address = Address::from(handler_address_bytes);
				let mut execution_context_address_bytes = [0u8; 20];
				execution_context_address_bytes.copy_from_slice(&data[92..112]);
				let execution_context_address = Address::from(execution_context_address_bytes);
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, tokenAddress: 20 bytes, to: 20 bytes, amountToRescue: 32 bytes)) = 112
	/// Bytes
	fn decode_rescue_tokens_proposal(data: &[u8]) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 112 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut token_address_bytes = [0u8; 20];
				token_address_bytes.copy_from_slice(&data[40..60]);
				let token_address = Address::from(token_address_bytes);
				let mut to_bytes = [0u8; 20];
				to_bytes.copy_from_slice(&data[60..80]);
				let to = Address::from(to_bytes);
				let mut amount_to_rescue_bytes = [0u8; 32];
				amount_to_rescue_bytes.copy_from_slice(&data[80..112]);
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	/// (header: 40 Bytes, min_withdrawal_limit_bytes: 32) = 72
	/// Bytes
	fn decode_configurable_limit_proposal(
		data: &[u8],
	) -> Result<ProposalHeader<T::ChainId>, Error<T>> {
		let header = Self::decode_proposal_header(data)?;
		match header.chain_id {
			ChainIdType::EVM(ChainId) => {
				if data.len() != 72 {
					return Err(Error::<T>::ProposalFormatInvalid)?
				}
				let mut configurable_limit_bytes = [0u8; 32];
				configurable_limit_bytes.copy_from_slice(&data[40..72]);
			},
			ChainIdType::Substrate(ChainId) => {},
			_ => {},
		}
		Ok(header)
	}

	#[cfg(feature = "runtime-benchmarks")]
	pub fn signed_proposals_len() -> usize {
		SignedProposals::<T>::iter_keys().collect::<Vec<_>>().len()
	}
}
