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

use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{gossip_messages::DKGSignedPayload, DKGPayloadKey, MaxProposalLength};

use webb_proposals::{Proposal, ProposalKind};

/// Get signed proposal
pub(crate) fn get_signed_proposal(
	finished_round: DKGSignedPayload,
	payload_key: DKGPayloadKey,
) -> Result<Option<Proposal<MaxProposalLength>>, DKGError> {
	match payload_key {
		DKGPayloadKey::RefreshProposal(_nonce) =>
			make_signed_proposal(ProposalKind::Refresh, finished_round),
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
pub(crate) fn make_signed_proposal(
	kind: ProposalKind,
	finished_round: DKGSignedPayload,
) -> Result<Option<Proposal<MaxProposalLength>>, DKGError> {
	let bounded_data = finished_round.payload.try_into().map_err(|_| DKGError::InputOutOfBounds)?;
	let bounded_signature =
		finished_round.signature.try_into().map_err(|_| DKGError::InputOutOfBounds)?;
	Ok(Some(Proposal::Signed { kind, data: bounded_data, signature: bounded_signature }))
}
