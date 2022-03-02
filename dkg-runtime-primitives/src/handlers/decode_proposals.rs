use crate::{
	handlers::{evm, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, Proposal, ProposalHeader, ProposalKind,
	ProposalNonce,
};
use codec::{alloc::string::ToString, Decode};

use super::substrate;

pub fn decode_proposal_header<C: ChainIdTrait>(
	data: &[u8],
) -> Result<ProposalHeader<C>, ValidationError> {
	let header = ProposalHeader::<C>::decode(&mut &data[..]).map_err(|_| {
		ValidationError::InvalidParameter("Failed to decode proposal header".to_string())
	})?;
	frame_support::log::debug!(
		target: "dkg_proposal_handler",
		"🕸️ Decoded Proposal Header: {:?} ({} bytes)",
		header,
		data.len(),
	);
	Ok(header)
}

pub fn decode_proposal<C: ChainIdTrait>(
	proposal: &Proposal,
) -> Result<(ChainIdType<C>, DKGPayloadKey), ValidationError> {
	// First parse if EVM tx proposal
	match proposal.kind() {
		ProposalKind::EVM =>
			return evm::evm_tx::create(&proposal.data())
				.map(|p| (p.chain_id, DKGPayloadKey::EVMProposal(p.nonce))),
		_ => {},
	}

	// Otherwise, begin parsing DKG proposal header
	let (chain_id, _): (ChainIdType<C>, ProposalNonce) =
		decode_proposal_header(proposal.data()).map(Into::into)?;

	match proposal.kind() {
		ProposalKind::AnchorCreate => match chain_id {
			ChainIdType::EVM(_) => panic!("should not exist"),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) => substrate::anchor_create::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::AnchorCreateProposal(p.header.nonce))),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::AnchorUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::anchor_update::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::AnchorUpdateProposal(p.header.nonce))),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) => substrate::anchor_update::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::AnchorUpdateProposal(p.header.nonce))),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::TokenAdd => match chain_id {
			ChainIdType::EVM(_) => evm::add_token_to_set::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::TokenAddProposal(p.header.nonce))),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) => substrate::add_token_to_pool_share::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::TokenAddProposal(p.header.nonce))),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::TokenRemove => match chain_id {
			ChainIdType::EVM(_) => evm::remove_token_from_set::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::TokenRemoveProposal(p.header.nonce))),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) =>
				substrate::remove_token_from_pool_share::create(&proposal.data()).map(|p| {
					(p.header.chain_id, DKGPayloadKey::TokenRemoveProposal(p.header.nonce))
				}),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::WrappingFeeUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::fee_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::WrappingFeeUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) => substrate::fee_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::WrappingFeeUpdateProposal(p.header.nonce))
			}),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::ResourceIdUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::resource_id_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::ResourceIdUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) |
			ChainIdType::RelayChain(_, _) |
			ChainIdType::Parachain(_, _) =>
				substrate::resource_id_update::create(&proposal.data()).map(|p| {
					(p.header.chain_id, DKGPayloadKey::ResourceIdUpdateProposal(p.header.nonce))
				}),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::RescueTokens => match chain_id {
			ChainIdType::EVM(_) => evm::rescue_tokens::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::RescueTokensProposal(p.header.nonce))),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::MaxDepositLimitUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::bytes32_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::MaxDepositLimitUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::MinWithdrawalLimitUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::bytes32_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::MinWithdrawalLimitUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::MaxExtLimitUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::bytes32_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::MaxExtLimitUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::MaxFeeLimitUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::bytes32_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::MaxFeeLimitUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::SetTreasuryHandler => match chain_id {
			ChainIdType::EVM(_) => evm::set_treasury_handler::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::SetTreasuryHandlerProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::SetVerifier => match chain_id {
			ChainIdType::EVM(_) => evm::set_verifier::create(&proposal.data())
				.map(|p| (p.header.chain_id, DKGPayloadKey::SetVerifierProposal(p.header.nonce))),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		ProposalKind::FeeRecipientUpdate => match chain_id {
			ChainIdType::EVM(_) => evm::fee_recipient_update::create(&proposal.data()).map(|p| {
				(p.header.chain_id, DKGPayloadKey::FeeRecipientUpdateProposal(p.header.nonce))
			}),
			ChainIdType::Substrate(_) => todo!(),
			ChainIdType::RelayChain(_, _) => todo!(),
			ChainIdType::Parachain(_, _) => todo!(),
			ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
			ChainIdType::Solana(_) => panic!("Unimplemented"),
		},
		_ => Err(ValidationError::UnimplementedProposalKind),
	}
}
