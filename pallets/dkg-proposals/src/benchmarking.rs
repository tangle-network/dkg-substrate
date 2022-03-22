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
use codec::Decode;
use dkg_runtime_primitives::{TypedChainId, ResourceId, ProposalNonce};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::RawOrigin;
use sp_std::{prelude::*, vec};
use sp_core::{
	ecdsa::{self, Signature},
	keccak_256, Pair, Public,
};
use  hex_literal::hex;
const SEED: u32 = 0;
const CHAIN_IDENTIFIER: u32 = 10;

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn assert_has_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_has_event(generic_event.into());
}

benchmarks! {

	set_threshold {
		let c in 1 .. 16_000;
		let admin = T::AdminOrigin::successful_origin();
	}: _<T::Origin>(admin, c as u32)
	verify {
		assert_last_event::<T>(Event::ProposerThresholdChanged { new_threshold: c}.into());
	}

	set_resource {
		let c in 1 .. 16_000;
		let admin = T::AdminOrigin::successful_origin();
		let mut resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; c as usize];
	}: _<T::Origin>(admin, resource_id, bytes)
	verify {
		assert!(Pallet::<T>::resource_exists(resource_id) == true);
	}

	remove_resource {
		let admin = T::AdminOrigin::successful_origin();
		let mut resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes);
	}: _<T::Origin>(admin, resource_id)
	verify {
		assert!(Pallet::<T>::resource_exists(resource_id) == false);
	}

	whitelist_chain {
		let admin = T::AdminOrigin::successful_origin();
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER.into());
		// let chain_id = ChainIdType::Substrate(CHAIN_IDENTIFIER);
	}: _<T::Origin>(admin, chain_id.clone())
	verify {
		assert_last_event::<T>(Event::ChainWhitelisted{ chain_id: chain_id.clone()}.into());
	}

	add_proposer {
		let admin = T::AdminOrigin::successful_origin();
		let v: T::AccountId = account("account", 0, SEED);
		let new_maintainer = hex!("8db55b05db86c0b1786ca49f095d76344c9e6056b2f02701a7e7f3c20aabfd913ebbe148dd17c56551a52952371071a6c604b3f3abe8f2c8fa742158ea6dd7d4");

	let pair = ecdsa::Pair::from_string(
		"0x9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
		None,
	)
	.unwrap();
	let old_maintainer =
		libsecp256k1::PublicKey::parse_compressed(&pair.public().0).unwrap().serialize()[1..]
			.to_vec();

	}: _<T::Origin>(admin, v.clone(), old_maintainer)
	verify {
		assert_last_event::<T>(Event::ProposerAdded{ proposer_id: v}.into());
	}

	remove_proposer {
		let admin = T::AdminOrigin::successful_origin();
		let v: T::AccountId = account("account", 0, SEED);

		let new_maintainer = hex!("8db55b05db86c0b1786ca49f095d76344c9e6056b2f02701a7e7f3c20aabfd913ebbe148dd17c56551a52952371071a6c604b3f3abe8f2c8fa742158ea6dd7d4");

	let pair = ecdsa::Pair::from_string(
		"0x9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
		None,
	)
	.unwrap();
	let old_maintainer =
		libsecp256k1::PublicKey::parse_compressed(&pair.public().0).unwrap().serialize()[1..]
			.to_vec();
		crate::Pallet::<T>::register_proposer(v.clone(), old_maintainer);
	}: _<T::Origin>(admin, v.clone())
	verify {
		assert_last_event::<T>(Event::ProposerRemoved{ proposer_id: v}.into());
	}

	acknowledge_proposal {
		let c in 1 .. 16_000;
		let caller: T::AccountId = whitelisted_caller();
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes);
		let nonce = 1;
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER.into());
		let bytes = vec![0u8; c as usize];
		let proposal_bytes: T::Proposal = T::Proposal::decode(&mut &bytes[..]).unwrap();
		Proposers::<T>::insert(caller.clone(), true);
		Pallet::<T>::whitelist(chain_id.clone());
		Pallet::<T>::set_proposer_threshold(10);
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			Proposers::<T>::insert(who.clone(), true);
			Pallet::<T>::commit_vote(who, i.into(), chain_id.clone(), &proposal_bytes, true);
		}
	}: _(RawOrigin::Signed(caller.clone()), nonce.into(), chain_id.clone(),  resource_id, proposal_bytes)
	verify {
		assert_last_event::<T>(Event::VoteFor{ chain_id: chain_id.clone(), proposal_nonce: nonce.into(), who: caller}.into());
	}

	reject_proposal {
		let c in 1 .. 16_000;
		let caller: T::AccountId = whitelisted_caller();
		let resource_id: ResourceId = [0; 32].into();
		let bytes = vec![0u8; 12];
		Pallet::<T>::register_resource(resource_id, bytes);
		let nonce = 1;
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER.into());
		let bytes = vec![0u8; c as usize];
		let bytes = vec![0u8; 12];
		let proposal_bytes: T::Proposal = T::Proposal::decode(&mut &bytes[..]).unwrap();
		Proposers::<T>::insert(caller.clone(), true);
		Pallet::<T>::whitelist(chain_id.clone());
		Pallet::<T>::set_proposer_threshold(10);
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			Proposers::<T>::insert(who.clone(), true);
			Pallet::<T>::commit_vote(who, i.into(), chain_id.clone(), &proposal_bytes, false);
		}
	}: _(RawOrigin::Signed(caller.clone()), nonce.into(), chain_id.clone(),  resource_id, proposal_bytes)
	verify {
		assert_last_event::<T>(Event::VoteAgainst{ chain_id: chain_id.clone(), proposal_nonce: nonce.into(), who: caller}.into());
	}

	eval_vote_state {
		let c in 1 .. 16_000;
		let caller: T::AccountId = whitelisted_caller();
		let bytes = vec![0u8; 12];
		let nonce: ProposalNonce = 1.into();
		let chain_id: TypedChainId = TypedChainId::Evm(CHAIN_IDENTIFIER.into());
		let bytes = vec![0u8; c as usize];
		let bytes = vec![0u8; 12];
		let proposal_bytes: T::Proposal = T::Proposal::decode(&mut &bytes[..]).unwrap();
		Proposers::<T>::insert(caller.clone(), true);
		Pallet::<T>::whitelist(chain_id.clone());
		Pallet::<T>::set_proposer_threshold(10);
		for i in 1..9 {
			let who: T::AccountId = account("account", i, SEED);
			Proposers::<T>::insert(who.clone(), true);
			Pallet::<T>::commit_vote(who, i.into(), chain_id.clone(), &proposal_bytes, false);
		}

		Pallet::<T>::commit_vote(caller.clone(), nonce, chain_id.clone(), &proposal_bytes, true);
	}: _(RawOrigin::Signed(caller.clone()), nonce, chain_id.clone(),  proposal_bytes.clone())
	verify {
		assert!(Votes::<T>::get(chain_id.clone(), (nonce, proposal_bytes)) != None);
	}
}
