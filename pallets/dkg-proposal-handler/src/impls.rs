use super::*;

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	type BatchId = T::BatchId;
	type MaxProposalLength = T::MaxProposalLength;
	type MaxProposals = T::MaxProposalsPerBatch;
	type MaxSignatureLen = T::MaxSignatureLength;

	fn handle_unsigned_proposal(proposal: Proposal<Self::MaxProposalLength>) -> DispatchResult {
		match decode_proposal_identifier(&proposal) {
			Ok(v) => {
				Self::deposit_event(Event::<T>::ProposalAdded {
					key: v.key,
					target_chain: v.typed_chain_id,
					data: proposal.data().clone(),
				});
				Self::store_unsigned_proposal(proposal, v)?;
				Ok(())
			},
			Err(e) => Err(Self::handle_validation_error(e).into()),
		}
	}

	fn handle_signed_proposal_batch(
		prop: SignedProposalBatch<
			Self::BatchId,
			Self::MaxProposalLength,
			Self::MaxProposals,
			Self::MaxSignatureLen,
		>,
	) -> DispatchResult {
		ensure!(!prop.proposals.is_empty(), Error::<T>::EmptyBatch);

		let id = match decode_proposal_identifier(
			prop.proposals.first().expect("Batch cannot be empty, checked above"),
		) {
			Ok(v) => v,
			Err(e) => return Err(Self::handle_validation_error(e).into()),
		};

		// Log the chain id and nonce
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: chain: {:?}, payload_key: {:?}",
			id.typed_chain_id,
			id.key,
		);

		ensure!(
			UnsignedProposalQueue::<T>::contains_key(id.typed_chain_id, prop.batch_id),
			Error::<T>::ProposalDoesNotExists
		);

		// Log that proposal exist in the unsigned queue
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: proposal exist in the unsigned queue"
		);

		ensure!(
			Self::validate_proposal_signature(&prop.data(), &prop.signature),
			Error::<T>::ProposalSignatureInvalid
		);
		// Log that the signature is valid
		log::debug!(
			target: "runtime::dkg_proposal_handler",
			"submit_signed_proposal: signature is valid"
		);

		// ensure we are not overwriting an existing signed proposal
		ensure!(
			SignedProposals::<T>::get(id.typed_chain_id, prop.batch_id).is_none(),
			Error::<T>::CannotOverwriteSignedProposal
		);

		// Update storage
		SignedProposals::<T>::insert(id.typed_chain_id, prop.batch_id, prop.clone());

		UnsignedProposalQueue::<T>::remove(id.typed_chain_id, prop.batch_id);

		// Emit RuntimeEvent so frontend can react to it.
		let signed_proposal_events = prop
			.proposals
			.iter()
			.map(|proposal| SignedProposalEventData {
				kind: proposal.kind(),
				data: proposal.data().clone(),
			})
			.collect::<Vec<_>>();
		Self::deposit_event(Event::<T>::ProposalBatchSigned {
			proposals: signed_proposal_events,
			target_chain: id.typed_chain_id,
			batch_id: prop.batch_id,
			signature: prop.signature.to_vec(),
		});

		// Finally let any handlers handle the signed proposal
		for proposal in prop.proposals.into_iter() {
			log::debug!(
				target: "runtime::dkg_proposal_handler",
				"submit_signed_proposal: Calling SignedProposalHandler for proposal"
			);
			// we dont care about the result here
			let _ = T::SignedProposalHandler::on_signed_proposal(proposal);
		}

		Ok(())
	}
}
