use crate::{ChainIdType, Proposal, ProposalKind};
use crate::handlers::evm;
use sp_runtime::traits::AtLeast32Bit;

pub fn validate_proposal<ChainId: AtLeast32Bit>(chain_id: ChainIdType<ChainId>, proposal: Proposal) -> bool {
    match proposal.kind() {
        ProposalKind::Refresh => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::EVM => match chain_id {
            ChainIdType::EVM(_) => evm::anchor_update::validate_anchor_update(&proposal.data()),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::AnchorUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::TokenAdd => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::TokenRemove => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::WrappingFeeUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::ResourceIdUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::RescueTokens => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::MaxDepositLimitUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::MinWithdrawalLimitUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::MaxExtLimitUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
        ProposalKind::MaxFeeLimitUpdate => match chain_id {
            ChainIdType::EVM(_) => todo!(),
            ChainIdType::Substrate(_) => todo!(),
            ChainIdType::RelayChain(_, _) => todo!(),
            ChainIdType::Parachain(_, _) => todo!(),
            ChainIdType::CosmosSDK(_) => panic!("Unimplemented"),
            ChainIdType::Solana(_) => panic!("Unimplemented"),
        },
    }
}