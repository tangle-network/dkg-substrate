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
use crate::{
	handlers::{evm, validate_proposals::ValidationError},
	DKGPayloadKey, Proposal, ProposalKind,
};

use super::substrate;

pub fn decode_proposal_header(
	data: &[u8],
) -> Result<webb_proposals::ProposalHeader, ValidationError> {
	if data.len() < webb_proposals::ProposalHeader::LENGTH {
		return Err(ValidationError::InvalidProposalBytesLength)
	}
	let mut bytes = [0u8; webb_proposals::ProposalHeader::LENGTH];
	bytes.copy_from_slice(data[..webb_proposals::ProposalHeader::LENGTH].as_ref());
	let header = webb_proposals::ProposalHeader::from(bytes);
	Ok(header)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ProposalIdentifier {
	pub key: DKGPayloadKey,
	pub typed_chain_id: webb_proposals::TypedChainId,
}

pub fn decode_proposal_identifier(
	proposal: &Proposal,
) -> Result<ProposalIdentifier, ValidationError> {
	// First parse if EVM tx proposal
	if let ProposalKind::EVM = proposal.kind() {
		return evm::evm_tx::create(proposal.data()).map(|p| ProposalIdentifier {
			key: DKGPayloadKey::EVMProposal(p.nonce),
			typed_chain_id: webb_proposals::TypedChainId::Evm(p.chain_id),
		})
	}

	// Otherwise, begin parsing DKG proposal header
	fn matches_kind(
		prop_kind: ProposalKind,
		expected_kind: ProposalKind,
	) -> impl Fn(ProposalIdentifier) -> Result<ProposalIdentifier, ValidationError> {
		move |out| {
			if prop_kind == expected_kind {
				Ok(out)
			} else {
				Err(ValidationError::UnimplementedProposalKind)
			}
		}
	}

	let header = decode_proposal_header(proposal.data())?;
	let mut identifier = ProposalIdentifier {
		key: DKGPayloadKey::EVMProposal(header.nonce()), // placeholder
		typed_chain_id: header.resource_id().typed_chain_id(),
	};

	// we then create a lazy identifier.
	let maybe_anchor_create = substrate::anchor_create::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::AnchorCreateProposal(p.header.nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::AnchorCreate));

	let maybe_substrate_anchor_update = substrate::anchor_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::AnchorUpdateProposal(p.header.nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::AnchorUpdate));

	let maybe_evm_anchor_update = evm::anchor_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::AnchorUpdateProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::AnchorUpdate));

	let maybe_evm_token_add = evm::add_token_to_set::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::TokenAddProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::TokenAdd));

	let maybe_substrate_token_add = substrate::add_token_to_pool_share::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::TokenAddProposal(p.header.nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::TokenAdd));

	let maybe_evm_token_remove = evm::remove_token_from_set::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::TokenRemoveProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::TokenRemove));

	let maybe_substrate_token_remove =
		substrate::remove_token_from_pool_share::create(proposal.data())
			.map(|p| {
				identifier.key = DKGPayloadKey::TokenRemoveProposal(p.header.nonce());
				identifier
			})
			.and_then(matches_kind(proposal.kind(), ProposalKind::TokenRemove));

	let maybe_evm_fee_update = evm::fee_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::WrappingFeeUpdateProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::WrappingFeeUpdate));

	let maybe_substrate_fee_update = substrate::fee_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::WrappingFeeUpdateProposal(p.header.nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::WrappingFeeUpdate));

	let maybe_evm_resoruce_id_update = evm::resource_id_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::ResourceIdUpdateProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::ResourceIdUpdate));

	let maybe_substrate_resource_id_update = substrate::resource_id_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::ResourceIdUpdateProposal(p.header.nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::ResourceIdUpdate));

	let maybe_evm_rescue_tokens = evm::rescue_tokens::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::RescueTokensProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::RescueTokens));

	let maybe_max_deposit_limit_update = evm::max_deposit_limit_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::MaxDepositLimitUpdateProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::MaxDepositLimitUpdate));

	let maybe_min_withdrawal_limit_update =
		evm::min_withdrawal_limit_update::create(proposal.data())
			.map(|p| {
				identifier.key =
					DKGPayloadKey::MinWithdrawalLimitUpdateProposal(p.header().nonce());
				identifier
			})
			.and_then(matches_kind(proposal.kind(), ProposalKind::MinWithdrawalLimitUpdate));

	let maybe_set_treasury_handler = evm::set_treasury_handler::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::SetTreasuryHandlerProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::SetTreasuryHandler));

	let maybe_set_verifier = evm::set_verifier::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::SetVerifierProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::SetVerifier));

	let maybe_fee_recipient_update = evm::fee_recipient_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::FeeRecipientUpdateProposal(p.header().nonce());
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::FeeRecipientUpdate));

	let maybe_proposer_set_update = super::proposer_set_update::create(proposal.data())
		.map(|p| {
			identifier.key = DKGPayloadKey::ProposerSetUpdateProposal(p.nonce);
			identifier.typed_chain_id = webb_proposals::TypedChainId::None;
			identifier
		})
		.and_then(matches_kind(proposal.kind(), ProposalKind::ProposerSetUpdate));

	// Switch on all cases
	maybe_evm_anchor_update
		.or(maybe_substrate_anchor_update)
		.or(maybe_evm_token_add)
		.or(maybe_substrate_token_add)
		.or(maybe_evm_token_remove)
		.or(maybe_substrate_token_remove)
		.or(maybe_evm_fee_update)
		.or(maybe_substrate_fee_update)
		.or(maybe_evm_resoruce_id_update)
		.or(maybe_substrate_resource_id_update)
		.or(maybe_evm_rescue_tokens)
		.or(maybe_max_deposit_limit_update)
		.or(maybe_min_withdrawal_limit_update)
		.or(maybe_set_treasury_handler)
		.or(maybe_set_verifier)
		.or(maybe_fee_recipient_update)
		.or(maybe_anchor_create)
		.or(maybe_proposer_set_update)
		.map_err(|_| ValidationError::UnimplementedProposalKind)
}
