// This file is part of Webb.

// Copyright (C) 2021 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{worker::DKGWorker, Client};
use codec::Encode;
use dkg_primitives::types::DKGSignedPayload;
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::OFFCHAIN_PUBLIC_KEY_SIG, DKGApi, DKGPayloadKey,
	Proposal, ProposalKind, RefreshProposalSigned,
};
use log::trace;
use sc_client_api::Backend;
use sp_api::offchain::STORAGE_PREFIX;
use sp_core::offchain::OffchainStorage;
use sp_runtime::traits::{Block, Header};

/// Get signed proposal
pub(crate) fn get_signed_proposal<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	finished_round: DKGSignedPayload,
	payload_key: DKGPayloadKey,
) -> Option<Proposal>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let signed_proposal = match payload_key {
		DKGPayloadKey::RefreshVote(nonce) => {
			let offchain = dkg_worker.backend.offchain_storage();

			if let Some(mut offchain) = offchain {
				let refresh_proposal =
					RefreshProposalSigned { nonce, signature: finished_round.signature.clone() };
				let encoded_proposal = refresh_proposal.encode();
				offchain.set(STORAGE_PREFIX, OFFCHAIN_PUBLIC_KEY_SIG, &encoded_proposal);

				trace!(target: "dkg", "Stored pub_key signature offchain {:?}", finished_round.signature);
			}

			return None
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
		DKGPayloadKey::MaxExtLimitUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::MaxExtLimitUpdate, finished_round),
		DKGPayloadKey::MaxFeeLimitUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::MaxFeeLimitUpdate, finished_round),
		DKGPayloadKey::SetVerifierProposal(_) =>
			make_signed_proposal(ProposalKind::SetVerifier, finished_round),
		DKGPayloadKey::SetTreasuryHandlerProposal(_) =>
			make_signed_proposal(ProposalKind::SetTreasuryHandler, finished_round),
		DKGPayloadKey::FeeRecipientUpdateProposal(_) =>
			make_signed_proposal(ProposalKind::FeeRecipientUpdate, finished_round),
	};

	signed_proposal
}

/// make an unsigned proposal a signed one
fn make_signed_proposal(kind: ProposalKind, finished_round: DKGSignedPayload) -> Option<Proposal> {
	Some(Proposal::Signed {
		kind,
		data: finished_round.payload,
		signature: finished_round.signature.clone(),
	})
}
