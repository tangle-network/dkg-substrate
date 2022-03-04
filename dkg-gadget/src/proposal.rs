use crate::{types::dkg_topic, worker::DKGWorker, Client};
use codec::Encode;
use dkg_primitives::{
	crypto::Public,
	types::{
		DKGError, DKGMessage, DKGMsgPayload, DKGPublicKeyMessage, DKGSignedPayload, RoundId,
		SignedDKGMessage,
	},
};
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::OFFCHAIN_PUBLIC_KEY_SIG, AggregatedPublicKeys,
	DKGApi, DKGPayloadKey, Proposal, ProposalKind, RefreshProposalSigned,
};
use log::{debug, error, trace};
use sc_client_api::Backend;
use sp_api::offchain::STORAGE_PREFIX;
use sp_core::offchain::OffchainStorage;
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header},
};

/// Get signed proposal
pub(crate) fn get_signed_proposal<B, C, BE>(
	mut dkg_worker: &mut DKGWorker<B, C, BE>,
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

/// makes a proposal kind signed
fn make_signed_proposal(kind: ProposalKind, finished_round: DKGSignedPayload) -> Option<Proposal> {
	Some(Proposal::Signed {
		kind,
		data: finished_round.payload,
		signature: finished_round.signature.clone(),
	})
}
