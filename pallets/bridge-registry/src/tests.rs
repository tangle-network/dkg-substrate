use super::*;
use crate::mock::*;
use frame_support::{assert_err, assert_ok, bounded_vec, BoundedVec};
use std::convert::TryFrom;
use webb_proposals::{self, evm, FunctionSignature, Nonce, ProposalHeader};

#[test]
fn should_init() {
	new_test_ext().execute_with(|| {
		assert_eq!(NextBridgeIndex::<Test>::get(), 1);
		assert_eq!(ResourceToBridgeIndex::<Test>::get(&ResourceId([0u8; 32])), None);
	});
}

#[test]
fn should_handle_signed_evm_anchor_update_proposals() {
	new_test_ext().execute_with(|| {
		// Create target info
		let target_chain = webb_proposals::TypedChainId::Evm(1);
		let target_system = webb_proposals::TargetSystem::new_contract_address([1u8; 20]);
		let target_resource_id = webb_proposals::ResourceId::new(target_system, target_chain);
		// Create src info
		let src_chain = webb_proposals::TypedChainId::Evm(2);
		let src_target_system = webb_proposals::TargetSystem::new_contract_address([2u8; 20]);
		let src_resource_id = webb_proposals::ResourceId::new(src_target_system, src_chain);
		// Create mocked signed EVM anchor update proposals
		let proposal = evm::AnchorUpdateProposal::new(
			ProposalHeader::new(target_resource_id, FunctionSignature([0u8; 4]), Nonce(1)),
			[1u8; 32],
			src_resource_id,
		);
		let signed_proposal = Proposal::Signed {
			kind: ProposalKind::AnchorUpdate,
			data: proposal.into_bytes().to_vec(),
			signature: vec![],
		};
		// Handle signed proposal
		assert_ok!(BridgeRegistry::on_signed_proposal(signed_proposal));
		// Verify the storage system updates correctly
		assert_eq!(ResourceToBridgeIndex::<Test>::get(&target_resource_id), Some(1));
		assert_eq!(ResourceToBridgeIndex::<Test>::get(&src_resource_id), Some(1));
		assert_eq!(
			Bridges::<Test>::get(1).unwrap(),
			BridgeMetadata {
				resource_ids: bounded_vec![target_resource_id, src_resource_id],
				info: Default::default()
			}
		);
		assert_eq!(NextBridgeIndex::<Test>::get(), 2);
	});
}

#[test]
fn should_handle_multiple_signed_evm_anchor_update_proposals() {
	new_test_ext().execute_with(|| {
		// Create target info
		let target_chain = webb_proposals::TypedChainId::Evm(1);
		let target_system = webb_proposals::TargetSystem::new_contract_address([1u8; 20]);
		let target_resource_id = webb_proposals::ResourceId::new(target_system, target_chain);
		// Create src info
		let mut resources = vec![target_resource_id];
		for i in 1..7 {
			let src_chain = webb_proposals::TypedChainId::Evm(i);
			let src_target_system =
				webb_proposals::TargetSystem::new_contract_address([i as u8; 20]);
			let src_resource_id = webb_proposals::ResourceId::new(src_target_system, src_chain);
			// Create mocked signed EVM anchor update proposals
			let proposal = evm::AnchorUpdateProposal::new(
				ProposalHeader::new(target_resource_id, FunctionSignature([0u8; 4]), Nonce(1)),
				[1u8; 32],
				src_resource_id,
			);
			let signed_proposal = Proposal::Signed {
				kind: ProposalKind::AnchorUpdate,
				data: proposal.into_bytes().to_vec(),
				signature: vec![],
			};
			assert_ok!(BridgeRegistry::on_signed_proposal(signed_proposal));
			resources.push(src_resource_id);
		}
		// Check that all resources point to the same bridge
		for r in resources.clone() {
			assert_eq!(ResourceToBridgeIndex::<Test>::get(&r), Some(1));
		}
		// Check that all resources are in the storage system as expected
		assert_eq!(
			Bridges::<Test>::get(1).unwrap(),
			BridgeMetadata {
				resource_ids: BoundedVec::try_from(resources).unwrap(),
				info: Default::default()
			}
		);
	});
}

#[test]
fn should_fail_to_link_resources_from_different_bridges() {
	new_test_ext().execute_with(|| {
		// Create target info
		let target_chain = webb_proposals::TypedChainId::Evm(1);
		let target_system = webb_proposals::TargetSystem::new_contract_address([1u8; 20]);
		let target_resource_id = webb_proposals::ResourceId::new(target_system, target_chain);
		{
			// Connect target with itself
			let src_dummy_proposal = evm::AnchorUpdateProposal::new(
				ProposalHeader::new(target_resource_id, FunctionSignature([0u8; 4]), Nonce(1)),
				[1u8; 32],
				target_resource_id,
			);
			assert_ok!(BridgeRegistry::on_signed_proposal(Proposal::Signed {
				kind: ProposalKind::AnchorUpdate,
				data: src_dummy_proposal.into_bytes().to_vec(),
				signature: vec![],
			}));
			assert_eq!(NextBridgeIndex::<Test>::get(), 2);
		}

		// Create src info
		let src_chain = webb_proposals::TypedChainId::Evm(2);
		let src_target_system = webb_proposals::TargetSystem::new_contract_address([2u8; 20]);
		let src_resource_id = webb_proposals::ResourceId::new(src_target_system, src_chain);
		{
			// Connect src with itself
			let dest_dummy_proposal = evm::AnchorUpdateProposal::new(
				ProposalHeader::new(src_resource_id, FunctionSignature([0u8; 4]), Nonce(1)),
				[1u8; 32],
				src_resource_id,
			);
			assert_ok!(BridgeRegistry::on_signed_proposal(Proposal::Signed {
				kind: ProposalKind::AnchorUpdate,
				data: dest_dummy_proposal.into_bytes().to_vec(),
				signature: vec![],
			}));
			assert_eq!(NextBridgeIndex::<Test>::get(), 3);
		}

		let bad_proposal = evm::AnchorUpdateProposal::new(
			ProposalHeader::new(target_resource_id, FunctionSignature([0u8; 4]), Nonce(1)),
			[1u8; 32],
			src_resource_id,
		);

		assert_err!(
			BridgeRegistry::on_signed_proposal(Proposal::Signed {
				kind: ProposalKind::AnchorUpdate,
				data: bad_proposal.into_bytes().to_vec(),
				signature: vec![],
			}),
			Error::<Test>::BridgeIndexError
		);
	});
}
