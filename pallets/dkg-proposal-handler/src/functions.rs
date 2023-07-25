use super::*;
use dkg_runtime_primitives::handlers::decode_proposals::ProposalIdentifier;
use sp_runtime::traits::{CheckedAdd, One};

impl<T: Config> Pallet<T> {
	// *** API methods ***

	pub fn get_unsigned_proposal_batches() -> Vec<StoredUnsignedProposalBatchOf<T>> {
		UnsignedProposalQueue::<T>::iter()
			.map(|(_typed_chain_id, _batch_id, stored_unsigned_proposal)| stored_unsigned_proposal)
			.collect()
	}

	/// Checks whether a signed proposal exists in the `SignedProposals` storage
	pub fn is_not_existing_proposal_batch(prop: &SignedProposalBatchOf<T>) -> bool {
		match decode_proposal_identifier(
			prop.proposals.first().expect("bactch should not be empty"),
		) {
			Ok(v) => !SignedProposals::<T>::contains_key(v.typed_chain_id, prop.batch_id),
			Err(_) => false,
		}
	}

	// ** Calculate the turn of authorities to submit transactions **
	// we use a simple round robin algorithm to determine who submits the proposal on-chain, this
	// avoids all the validators trying to submit at the same time.
	pub(crate) fn get_expected_signer(block_number: T::BlockNumber) -> Option<T::AccountId> {
		let current_authorities = pallet_dkg_metadata::CurrentAuthoritiesAccounts::<T>::get();
		let block_as_u32: u32 = block_number.try_into().unwrap_or_default();

		// sanity check
		if current_authorities.is_empty() || block_as_u32.is_zero() {
			// we can safely return None here,
			// the calling function will submit the proposal anyway
			return None
		}

		let submitter_index: u32 = block_as_u32 % current_authorities.len() as u32;
		current_authorities.get(submitter_index as usize).cloned()
	}

	pub(crate) fn generate_next_batch_id() -> Result<T::BatchId, DispatchError> {
		let batch_id = Self::next_batch_id();
		let next_batch_id =
			batch_id.checked_add(&One::one()).ok_or(Error::<T>::ArithmeticOverflow)?;
		NextBatchId::<T>::put(next_batch_id);
		Ok(batch_id)
	}

	pub(crate) fn create_batch_and_add_to_storage(
		proposals: BoundedVec<UnsignedProposalOf<T>, T::MaxProposalsPerBatch>,
		identifier: ProposalIdentifier,
	) -> DispatchResult {
		// create a new batch
		let current_block = <frame_system::Pallet<T>>::block_number();
		let batch_id = Self::generate_next_batch_id()?;
		let batch =
			StoredUnsignedProposalBatchOf::<T> { proposals, timestamp: current_block, batch_id };

		// push the batch to unsigned proposal queue
		UnsignedProposalQueue::<T>::insert(identifier.typed_chain_id, batch_id, batch);
		Ok(())
	}

	// the goal of this function is to add an unsigned proposal to the staging unsigned storage.
	// but the unsigned storage has to be cleared if the limit has been reached
	pub(crate) fn store_unsigned_proposal(
		prop: ProposalOf<T>,
		identifier: ProposalIdentifier,
	) -> DispatchResult {
		let new_unsigned_proposal = UnsignedProposalOf::<T> {
			proposal: prop,
			key: identifier.key,
			typed_chain_id: identifier.typed_chain_id,
		};

		UnsignedProposals::<T>::try_mutate(identifier.typed_chain_id, |proposals| {
			let proposals = proposals.get_or_insert_with(Default::default);

			// if the bounded vec is full, we create a new batch with the current bounded vec
			// and push it to the UnsignedProposalQueue
			if proposals.len() == T::MaxProposalsPerBatch::get() as usize {
				// push the batch to unsigned proposal queue
				Self::create_batch_and_add_to_storage(proposals.clone(), identifier)?;

				let mut new_proposals: BoundedVec<_, _> = Default::default();

				new_proposals
					.try_push(new_unsigned_proposal)
					.map_err(|_| Error::<T>::UnsignedProposalQueueOverflow)?;

				*proposals = new_proposals;

				return Ok(())
			}

			proposals
				.try_push(new_unsigned_proposal)
				.map_err(|_| Error::<T>::UnsignedProposalQueueOverflow)?;

			Ok(())
		})
	}

	// report an offence against current active validator set
	pub fn report_offence(
		offence_type: DKGMisbehaviorOffenceType,
	) -> Result<(), sp_staking::offence::OffenceError> {
		let session_index = T::ValidatorSet::session_index();

		// The current dkg authorities are the same as the current validator set
		// so we pick the current validator set, this results in easier
		// conversion to pallet offences index
		let current_validators = T::ValidatorSet::validators();
		let offenders = current_validators
			.into_iter()
			.enumerate()
			.filter_map(|(_, id)| {
				<T::ValidatorSet as ValidatorSetWithIdentification<T::AccountId>>::IdentificationOf::convert(
					id.clone()
				).map(|full_id| (id, full_id))
			})
			.collect::<Vec<IdentificationTuple<T>>>();

		// we report an offence against the current DKG authorities
		let offence = DKGMisbehaviourOffence {
			offence: offence_type,
			session_index,
			validator_set_count: offenders.len() as u32,
			offenders,
		};
		T::ReportOffences::report_offence(sp_std::vec![], offence)
	}

	// *** Offchain worker methods ***

	/// Offchain worker function that submits signed proposals from the offchain storage on-chain
	///
	/// The function submits batches of signed proposals on-chain in batches of
	/// `T::MaxProposalsPerBatch`. Proposals are stored offchain and target specific block numbers
	/// for submission. This function polls all relevant proposals ready for submission at the
	/// current block number
	pub(crate) fn submit_signed_proposal_onchain(
		block_number: T::BlockNumber,
	) -> Result<(), &'static str> {
		// ensure we have the signer setup
		let signer = Signer::<T, <T as Config>::OffChainAuthId>::all_accounts();
		if !signer.can_sign() {
			return Err(
				"No local accounts available. Consider adding one via `author_insertKey` RPC.",
			)
		}

		// check if its our turn to submit proposals
		if let Some(expected_signer_account) = Self::get_expected_signer(block_number) {
			// the signer does not have a method to read all available public keys, we instead sign
			// a dummy message and read the current pub key from the signature.
			let signature = signer.sign_message(b"test");
			let account: &T::AccountId =
				&signature.first().expect("Unable to retreive signed message").0.id; // the unwrap here is ok since we checked if can_sign() is true above

			if account != &expected_signer_account {
				log::debug!(
					target: "runtime::dkg_proposal_handler",
					"submit_signed_proposal_onchain: Not our turn to sign, selected signer is {:?}",
					expected_signer_account
				);
				return Ok(())
			}
		}

		let mut lock = StorageLock::<Time>::new(SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK);
		{
			let _guard = lock.lock();

			match Self::get_next_offchain_signed_proposals() {
				Ok(next_proposals) => {
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"submit_signed_proposal_onchain: found {} proposal batches to submit before filtering\n {:?}",
						next_proposals.len(), next_proposals
					);

					// early exit if nothing to submit
					if next_proposals.is_empty() {
						return Ok(())
					}

					// We filter out all proposals that are already on chain
					let filtered_proposals = next_proposals
						.iter()
						.cloned()
						.filter(Self::is_not_existing_proposal_batch)
						.collect::<Vec<_>>();
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"submit_signed_proposal_onchain: found {} proposals to submit after filtering\n {:?}",
						filtered_proposals.len(), filtered_proposals
					);
					// We split the vector into chunks of `T::MaxProposalsPerBatch` length and
					// submit those chunks
					for chunk in filtered_proposals.chunks(T::MaxProposalsPerBatch::get() as usize)
					{
						let call = Call::<T>::submit_signed_proposals { props: chunk.to_vec() };
						let result = signer
							.send_signed_transaction(|_| call.clone())
							.into_iter()
							.map(|(_, r)| r)
							.collect::<Result<Vec<_>, _>>()
							.map_err(|()| "Unable to submit unsigned transaction.");
						// Display error if the signed tx fails.
						if result.is_err() {
							log::error!(
								target: "runtime::dkg_proposal_handler",
								"failure: failed to send unsigned transaction to chain: {:?}",
								call,
							);
						} else {
							// log the result of the transaction submission
							log::debug!(
								target: "runtime::dkg_proposal_handler",
								"Submitted unsigned transaction for signed proposal: {:?}",
								call,
							);
						}
					}
				},
				Err(e) => {
					log::trace!(
						target: "runtime::dkg_proposal_handler",
						"Failed to get next signed proposal: {}",
						e
					);
				},
			};
			Ok(())
		}
	}

	/// Returns the list of signed proposals ready for on-chain submission
	#[allow(clippy::type_complexity)]
	pub(crate) fn get_next_offchain_signed_proposals(
	) -> Result<Vec<SignedProposalBatchOf<T>>, &'static str> {
		let proposals_ref = StorageValueRef::persistent(OFFCHAIN_SIGNED_PROPOSALS);

		let mut all_proposals = Vec::new();
		let res = proposals_ref.mutate::<OffchainSignedProposalBatches<
			T::BatchId,
			T::MaxProposalLength,
			T::MaxProposalsPerBatch,
			T::MaxSignatureLength,
		>, _, _>(|res| {
			match res {
				Ok(Some(prop_wrapper)) => {
					// log the proposals
					log::debug!(
						target: "runtime::dkg_proposal_handler",
						"Offchain signed proposals: {:?}",
						prop_wrapper
					);
					all_proposals = prop_wrapper.clone().batches;
					Ok(prop_wrapper)
				},
				Ok(None) => Err("No signed proposals key stored"),
				Err(e) => {
					// log the error
					log::warn!(
						target: "runtime::dkg_proposal_handler",
						"Failed to read offchain signed proposals: {:?}",
						e
					);
					Err("Error decoding offchain signed proposals")
				},
			}
		});

		if res.is_err() {
			return Err("Unable to get next proposal batch")
		}

		Ok(all_proposals)
	}

	// *** Validation methods ***

	pub(crate) fn validate_proposal_signature(data: &[u8], signature: &[u8]) -> bool {
		dkg_runtime_primitives::utils::validate_ecdsa_signature(data, signature)
	}

	pub(crate) fn handle_validation_error(error: ValidationError) -> Error<T> {
		match error {
			ValidationError::InvalidParameter(_) => Error::<T>::ProposalFormatInvalid,
			ValidationError::UnimplementedProposalKind => Error::<T>::ProposalFormatInvalid,
			ValidationError::InvalidProposalBytesLength => Error::<T>::InvalidProposalBytesLength,
			ValidationError::InvalidDecoding(_) => Error::<T>::ProposalFormatInvalid,
		}
	}

	// *** Utility methods ***

	#[cfg(feature = "runtime-benchmarks")]
	pub fn signed_proposals_len() -> usize {
		SignedProposals::<T>::iter_keys().count()
	}
}
