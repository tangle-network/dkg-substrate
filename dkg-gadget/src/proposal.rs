use std::sync::Arc;
// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use crate::{debug_logger::DebugLogger, Client};
use codec::Encode;
use dkg_primitives::types::{DKGError, DKGSignedPayload};
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::OFFCHAIN_PUBLIC_KEY_SIG, DKGApi, DKGPayloadKey,
	RefreshProposalSigned,
};
use sc_client_api::Backend;
use sp_api::offchain::STORAGE_PREFIX;
use sp_core::offchain::OffchainStorage;
use sp_runtime::traits::{Block, Get, Header};
use webb_proposals::{Proposal, ProposalKind};

/// Get signed proposal
pub(crate) fn get_signed_proposal<B, C, BE, MaxProposalLength, MaxAuthorities>(
	backend: &Arc<BE>,
	finished_round: DKGSignedPayload,
	payload_key: DKGPayloadKey,
	logger: &DebugLogger,
) -> Result<Option<Proposal<MaxProposalLength>>, DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	C::Api: DKGApi<
		B,
		AuthorityId,
		<<B as Block>::Header as Header>::Number,
		MaxProposalLength,
		MaxAuthorities,
	>,
{
	match payload_key {
		DKGPayloadKey::RefreshVote(nonce) => {
			logger.info(format!("🕸️  Refresh vote with nonce {nonce:?} received"));
			let offchain = backend.offchain_storage();

			if let Some(mut offchain) = offchain {
				let refresh_proposal =
					RefreshProposalSigned { nonce, signature: finished_round.signature.clone() };
				let encoded_proposal = refresh_proposal.encode();
				offchain.set(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG, &encoded_proposal);

				logger.trace(format!(
					"Stored pub_key signature offchain {:?}",
					finished_round.signature
				));
			}

			Ok(None)
		},
		DKGPayloadKey::ProposerSetUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::ProposerSetUpdate, finished_round),
		DKGPayloadKey::EVMProposal(_) => make_signed_proposal(ProposalKind::EVM, finished_round),
		DKGPayloadKey::AnchorCreateProposal(_) =>
			make_signed_proposal(ProposalKind::AnchorCreate, finished_round),
		DKGPayloadKey::AnchorUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::AnchorUpdate, finished_round),
		DKGPayloadKey::TokenAddProposal(_) =>
			make_signed_proposal(ProposalKind::TokenAdd, finished_round),
		DKGPayloadKey::TokenRemoveProposal(_) =>
			make_signed_proposal(ProposalKind::TokenRemove, finished_round),
		DKGPayloadKey::WrappingFeeUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::WrappingFeeUpdate, finished_round),
		DKGPayloadKey::ResourceIdUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::ResourceIdUpdate, finished_round),
		DKGPayloadKey::RescueTokensProposal(_) =>
			make_signed_proposal(ProposalKind::RescueTokens, finished_round),
		DKGPayloadKey::MaxDepositLimitUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::MaxDepositLimitUpdate, finished_round),
		DKGPayloadKey::MinWithdrawalLimitUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::MinWithdrawalLimitUpdate, finished_round),
		DKGPayloadKey::SetVerifierProposal(_) =>
			make_signed_proposal(ProposalKind::SetVerifier, finished_round),
		DKGPayloadKey::SetTreasuryHandlerProposal(_) =>
			make_signed_proposal(ProposalKind::SetTreasuryHandler, finished_round),
		DKGPayloadKey::FeeRecipientUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::FeeRecipientUpdate, finished_round),
	}
}

/// make an unsigned proposal a signed one
pub(crate) fn make_signed_proposal<
	MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
>(
	kind: ProposalKind,
	finished_round: DKGSignedPayload,
) -> Result<Option<Proposal<MaxProposalLength>>, DKGError> {
	let bounded_data = finished_round.payload.try_into().map_err(|_| DKGError::InputOutOfBounds)?;
	let bounded_signature =
		finished_round.signature.try_into().map_err(|_| DKGError::InputOutOfBounds)?;
	Ok(Some(Proposal::Signed { kind, data: bounded_data, signature: bounded_signature }))
}
