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
//! Benchmarking for dkg-proposals
use super::*;

#[allow(unused)]
use crate::Pallet;
use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	proposal::Proposal, DKGPayloadKey, FunctionSignature, ProposalHeader, ProposalKind,
	ProposalNonce, TypedChainId,
};
use frame_benchmarking::{account, benchmarks, whitelisted_caller};
use frame_system::RawOrigin;
use sp_std::vec;
const SEED: u32 = 0;
const CHAIN_IDENTIFIER: u32 = 10;

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

pub fn make_proposal<T: Config>(
	prop: Proposal<<T as Config>::MaxProposalLength>,
) -> Proposal<<T as Config>::MaxProposalLength> {
	// Create the proposal Header
	let r_id = ResourceId::from([
		1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
		0, 1,
	]);
	let function_signature = FunctionSignature::from([0x26, 0x57, 0x88, 0x01]);
	let nonce = ProposalNonce::from(1);
	let header = ProposalHeader::new(r_id, function_signature, nonce);
	let mut buf = vec![];
	header.encode_to(&mut buf);
	// N bytes parameter
	buf.extend_from_slice(&[0u8; 64]);

	match prop {
		Proposal::Unsigned { kind: ProposalKind::AnchorUpdate, .. } =>
			Proposal::<<T as Config>::MaxProposalLength>::Unsigned {
				kind: ProposalKind::AnchorUpdate,
				data: buf.try_into().unwrap(),
			},
		_ => panic!("Invalid proposal type"),
	}
}

benchmarks! {

	set_threshold {
		let c in 1 .. 500;
		let admin = RawOrigin::Root;
	}: _(admin, c as u32)
	verify {
		assert_last_event::<T>(Event::ProposerThresholdChanged { new_threshold: c}.into());
	}

	set_resource {
		let c in 1 .. 500;
		let admin = RawOrigin::Root;
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; c as usize];
	}: _(admin, resource_id, bytes)
	verify {
		assert!(Pallet::<T>::resource_exists(resource_id));
	}

	remove_resource {
		let admin = RawOrigin::Root;
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes).unwrap();
	}: _(admin, resource_id)
	verify {
		assert!(!Pallet::<T>::resource_exists(resource_id));
	}

	whitelist_chain {
		let admin = RawOrigin::Root;
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER);
		// let chain_id = ChainIdType::Substrate(CHAIN_IDENTIFIER);
	}: _(admin, chain_id)
	verify {
		assert_last_event::<T>(Event::ChainWhitelisted{ chain_id}.into());
	}

	acknowledge_proposal {
		let c in 1 .. 500;
		let caller: T::AccountId = whitelisted_caller();
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes).unwrap();
		let nonce = 1;
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER);
		let bytes = vec![0u8; c as usize];
		let proposal = make_proposal::<T>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![].try_into().unwrap(),
		});
		let mut proposers: BoundedVec<T::AccountId, T::MaxProposers> = vec![caller.clone()].try_into().expect("Failed to create proposers");
		Proposers::<T>::put(proposers.clone());
		Pallet::<T>::whitelist(chain_id).unwrap();
		Pallet::<T>::set_proposer_threshold(10).unwrap();
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			proposers.try_push(who.clone()).expect("Failed to push proposer");
			Proposers::<T>::put(proposers.clone());
			Pallet::<T>::commit_vote(who, i.into(), chain_id, &proposal, true).unwrap();
		}
	}: _(RawOrigin::Signed(caller.clone()), nonce.into(), chain_id,  resource_id, proposal.clone())
	verify {
		assert_last_event::<T>(Event::VoteFor{ src_chain_id: chain_id, proposal_nonce: nonce.into(), who: caller, kind: proposal.kind()}.into());
	}

	reject_proposal {
		let c in 1 .. 500;
		let caller: T::AccountId = whitelisted_caller();
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes).unwrap();
		let nonce = 1;
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER);
		let bytes = vec![0u8; c as usize];
		let bytes = vec![0u8; 12];
		let proposal = make_proposal::<T>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![].try_into().unwrap(),
		});
		let mut proposers: BoundedVec<T::AccountId, T::MaxProposers> = vec![caller.clone()].try_into().expect("Failed to create proposers");
		Proposers::<T>::put(proposers.clone());
		Pallet::<T>::whitelist(chain_id).unwrap();
		Pallet::<T>::set_proposer_threshold(10).unwrap();
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			proposers.try_push(who.clone()).expect("Failed to push proposer");
			Proposers::<T>::put(proposers.clone());
			Pallet::<T>::commit_vote(who, i.into(), chain_id, &proposal, false).unwrap();
		}
	}: _(RawOrigin::Signed(caller.clone()), nonce.into(), chain_id,  resource_id, proposal.clone())
	verify {
		assert_last_event::<T>(Event::VoteAgainst{ src_chain_id: chain_id, proposal_nonce: nonce.into(), who: caller,kind: proposal.kind()}.into());
	}

	eval_vote_state {
		let c in 1 .. 500;
		let caller: T::AccountId = whitelisted_caller();
		let bytes = vec![0u8; 12];
		let nonce: ProposalNonce = 1.into();
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER);
		let bytes = vec![0u8; c as usize];
		let bytes = vec![0u8; 12];
		let proposal = make_proposal::<T>(Proposal::Unsigned {
			kind: ProposalKind::AnchorUpdate,
			data: vec![].try_into().unwrap(),
		});
		let mut proposers: BoundedVec<T::AccountId, T::MaxProposers> = vec![caller.clone()].try_into().expect("Failed to create proposers");
		Proposers::<T>::put(proposers.clone());
		Pallet::<T>::whitelist(chain_id).unwrap();
		Pallet::<T>::set_proposer_threshold(10).unwrap();
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			proposers.try_push(who.clone()).expect("Failed to push proposer");
			Proposers::<T>::put(proposers.clone());
			Pallet::<T>::commit_vote(who, i.into(), chain_id, &proposal, false).unwrap();
		}

		Pallet::<T>::commit_vote(caller.clone(), nonce, chain_id, &proposal, true).unwrap();
	}: _(RawOrigin::Signed(caller.clone()), nonce, chain_id,  proposal.clone())
	verify {
		assert!(Votes::<T>::get(chain_id, (nonce, proposal)).is_some());
	}
}
