use dkg_runtime_primitives::{
	traits::BridgeRegistryTrait, FunctionSignature, ProposalHandlerTrait, ProposalHeader,
	ProposalNonce, ResourceId, TypedChainId,
};
use sp_runtime::{traits::Get, DispatchError};
use webb_proposals::{evm, substrate, TargetSystem};

use crate::{evm_utils::parse_evm_log, proof::StorageVerifier, types::ProofData, Config, Error};

pub trait AnchorUpdateSubmitter<T: Config> {
	type ProposalHandler: ProposalHandlerTrait;
	type BridgeRegistry: BridgeRegistryTrait;

	fn create_anchor_update_data(
		typed_chain_id: TypedChainId,
		data: Vec<u8>,
	) -> Result<(ResourceId, [u8; 32], ProposalNonce), DispatchError> {
		match typed_chain_id {
			TypedChainId::Evm(..) => {
				let (contract_address, merkle_root, nonce) = parse_evm_log(data);
				Ok((
					ResourceId::new(
						TargetSystem::ContractAddress(contract_address),
						typed_chain_id,
					),
					merkle_root,
					ProposalNonce::from(nonce),
				))
			},
			_ => return Err(Error::<T>::InvalidTypedChainId)?,
		}
	}

	fn create_anchor_update_proposal_bytes(
		target_resource_id: ResourceId,
		proposal_nonce: ProposalNonce,
		merkle_root: [u8; 32],
		src_resource_id: ResourceId,
	) -> Result<Vec<u8>, DispatchError> {
		let function_sig = match target_resource_id.typed_chain_id() {
			TypedChainId::Evm(..) =>
				FunctionSignature::new(T::AnchorUpdateFunctionSignature::get()),
			_ => FunctionSignature::new([0u8; 4]),
		};
		let proposal_header = ProposalHeader::new(target_resource_id, function_sig, proposal_nonce);
		let evm_anchor_update_proposal: evm::AnchorUpdateProposal =
			evm::AnchorUpdateProposal::new(proposal_header, merkle_root, src_resource_id);
		let proposal_bytes = match src_resource_id.typed_chain_id() {
			TypedChainId::Evm(..) => evm_anchor_update_proposal.to_bytes().to_vec(),
			TypedChainId::Substrate(..) =>
				substrate::AnchorUpdateProposal::from(evm_anchor_update_proposal)
					.to_bytes()
					.to_vec(),
			_ => return Err(Error::<T>::InvalidTypedChainId)?,
		};

		Ok(proposal_bytes)
	}

	fn submit_anchor_update(
		typed_chain_id: TypedChainId,
		data: Vec<u8>,
	) -> Result<(), DispatchError> {
		let (src_resource_id, merkle_root, nonce) =
			Self::create_anchor_update_data(typed_chain_id, data)?;

		let bridge_resource_ids = T::BridgeRegistry::get_bridge_for_resource(src_resource_id);
		for linked_resource_id in bridge_resource_ids.unwrap_or_default() {
			if linked_resource_id == src_resource_id {
				continue
			}
			let proposal_bytes = Self::create_anchor_update_proposal_bytes(
				linked_resource_id,
				nonce,
				merkle_root,
				src_resource_id,
			)?;
			Self::ProposalHandler::handle_unsigned_proposal(
				proposal_bytes,
				dkg_runtime_primitives::ProposalAction::Sign(0),
			)?;
		}

		Ok(())
	}
}

pub trait AnchorUpdateCreator<T: Config> {
	type StorageVerifier: StorageVerifier<T>;
	type AnchorUpdateSubmitter: AnchorUpdateSubmitter<T>;

	fn create_anchor_update(
		typed_chain_id: TypedChainId,
		proof: ProofData,
	) -> Result<(), DispatchError> {
		match typed_chain_id {
			TypedChainId::Evm(_) => {
				let data = Self::StorageVerifier::verify_trie_proof(typed_chain_id, proof)?;
				// Submit the AnchorUpdateProposal
				Self::AnchorUpdateSubmitter::submit_anchor_update(typed_chain_id, data)?;
				Ok(().into())
			},
			_ => return Err(Error::<T>::InvalidTypedChainId)?,
		}
	}
}
