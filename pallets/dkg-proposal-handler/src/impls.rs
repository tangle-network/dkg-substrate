use super::*;
use dkg_runtime_primitives::{
	handlers::decode_proposals::ProposalIdentifier, ProposalKind::Refresh,
};
use sp_std::vec;

impl<T: Config> ProposalHandlerTrait for Pallet<T> {
	type BatchId = T::BatchId;
	type MaxProposalLength = T::MaxProposalLength;
	type MaxProposals = T::MaxProposalsPerBatch;
	type MaxSignatureLen = T::MaxSignatureLength;

	fn handle_unsigned_proposal(proposal: Vec<u8>, _action: ProposalAction) -> DispatchResult {
		let bounded_proposal: BoundedVec<_, _> =
			proposal.try_into().map_err(|_| Error::<T>::ProposalOutOfBounds)?;
		let proposal =
			Proposal::Unsigned { data: bounded_proposal, kind: ProposalKind::AnchorUpdate };

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

	fn handle_unsigned_proposer_set_update_proposal(
		proposal: Vec<u8>,
		_action: ProposalAction,
	) -> DispatchResult {
		let bounded_proposal: BoundedVec<_, _> =
			proposal.try_into().map_err(|_| Error::<T>::ProposalOutOfBounds)?;
		let unsigned_proposal =
			Proposal::Unsigned { data: bounded_proposal, kind: ProposalKind::ProposerSetUpdate };

		match decode_proposal_identifier(&unsigned_proposal) {
			Ok(v) => {
				// we create a batch and directly insert to the UnsignedProposalQueue
				// we dont bother adding to unsignedproposals since ProposerSetUpdate proposal
				// should always be a batch of one
				let batch_proposal: BoundedVec<_, _> = vec![UnsignedProposalOf::<T> {
					typed_chain_id: v.typed_chain_id.clone(),
					key: v.key.clone(),
					proposal: unsigned_proposal.clone(),
				}]
				.try_into()
				.map_err(|_| Error::<T>::UnsignedProposalQueueOverflow)?;

				Self::create_batch_and_add_to_storage(batch_proposal, v)?;

				Self::deposit_event(Event::<T>::ProposalAdded {
					key: v.key,
					target_chain: v.typed_chain_id,
					data: unsigned_proposal.data().clone(),
				});
				Ok(())
			},
			Err(e) => Err(Self::handle_validation_error(e).into()),
		}
	}

	fn handle_unsigned_refresh_proposal(
		proposal: dkg_runtime_primitives::RefreshProposal,
	) -> DispatchResult {
		let bounded_proposal: BoundedVec<_, _> =
			proposal.encode().try_into().map_err(|_| Error::<T>::ProposalOutOfBounds)?;
		let unsigned_proposal =
			Proposal::Unsigned { data: bounded_proposal, kind: ProposalKind::Refresh };

		let identifier = ProposalIdentifier {
			key: DKGPayloadKey::RefreshVote(proposal.nonce),
			typed_chain_id: TypedChainId::None,
		};

		// we create a batch and directly insert to the UnsignedProposalQueue
		// we dont bother adding to unsignedproposals since refresh proposal
		// should always be a batch of one
		let batch_proposal: BoundedVec<_, _> = vec![UnsignedProposalOf::<T> {
			typed_chain_id: identifier.typed_chain_id,
			key: identifier.key,
			proposal: unsigned_proposal.clone(),
		}]
		.try_into()
		.map_err(|_| Error::<T>::UnsignedProposalQueueOverflow)?;

		Self::create_batch_and_add_to_storage(batch_proposal, identifier)?;

		Self::deposit_event(Event::<T>::ProposalAdded {
			key: identifier.key,
			target_chain: identifier.typed_chain_id,
			data: unsigned_proposal.data().clone(),
		});

		Ok(())
	}

	fn handle_signed_refresh_proposal(
		_proposal: dkg_runtime_primitives::RefreshProposal,
		signature: Vec<u8>,
	) -> DispatchResult {
		// Attempt to remove all previous unsigned refresh proposals too
		// This may also remove ProposerSetUpdate proposals that haven't been signed
		// yet, but given that this action is only to clean storage when a refresh
		// fails, we can assume that the previous proposer set update will nonetheless
		// need to be used to update the governors on the respective webb Apps anyway.

		// TODO : process this better, there could be large amount of proposals other that refresh
		// vote with chainid none
		let possible_refresh_proposals =
			UnsignedProposalQueue::<T>::iter_prefix(TypedChainId::None);

		// This loop assumes that all proposals in UnsignedProposalQueue with TypedChainId::None is
		// a batch of size one, which is true for our case. Refactor this loop if this assumption is
		// no longer true.
		for (batch_id, proposal_batch) in possible_refresh_proposals {
			for proposal in proposal_batch.proposals.iter() {
				if proposal.proposal.kind() == Refresh {
					UnsignedProposalQueue::<T>::remove(TypedChainId::None, batch_id);

					let raw_proposal = &proposal_batch
						.proposals
						.first()
						.expect("should never happen since len is one")
						.proposal;

					// create signed batch from unsigned batch
					let signed_batch = SignedProposalBatchOf::<T> {
						batch_id,
						proposals: vec![raw_proposal.clone()]
							.try_into()
							.expect("should never happen since len is one"),
						signature: signature
							.clone()
							.try_into()
							.expect("proposal signature already checked!"),
					};

					// emit an event for the signed refresh proposal
					Self::deposit_event(Event::<T>::ProposalBatchSigned {
						target_chain: TypedChainId::None,
						proposals: signed_batch.clone(),
						data: signed_batch.data(),
						signature: signature
							.clone()
							.try_into()
							.expect("proposal signature already checked!"),
					});
				}
			}
		}

		Ok(())
	}

	fn handle_signed_proposal_batch(
		prop: SignedProposalBatch<
			Self::BatchId,
			Self::MaxProposalLength,
			Self::MaxProposals,
			Self::MaxSignatureLen,
		>,
	) -> DispatchResult {
		let id = match decode_proposal_identifier(prop.proposals.first().unwrap()) {
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
		Self::deposit_event(Event::<T>::ProposalBatchSigned {
			proposals: prop.clone(),
			target_chain: id.typed_chain_id,
			data: prop.data().to_vec(),
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
