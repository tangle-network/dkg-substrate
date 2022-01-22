//! Benchmarking for dkg-proposals
//!
use super::*;

#[allow(unused)]
use crate::Pallet;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller, account};
use frame_system::RawOrigin;
use dkg_runtime_primitives::ResourceId;
use sp_std::prelude::*;
use codec::Decode;

const SEED: u32 = 0;
const CHAIN_IDENTIFIER: u32 = 10;

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

benchmarks! {
	set_maintainer {
		let caller: T::AccountId = whitelisted_caller();

		let new_maintainer: T::AccountId  = account("account", 0, SEED);

		Maintainer::<T>::put(caller.clone());

	}: _(RawOrigin::Signed(caller.clone()), new_maintainer.clone())
	verify {
		assert_last_event::<T>(Event::MaintainerSet{ old_maintainer: Some(caller), new_maintainer: new_maintainer }.into());
	}

	force_set_maintainer {
		let admin = T::AdminOrigin::successful_origin();

		let maintainer: T::AccountId = account("account", 0, SEED);

		let new_maintainer: T::AccountId  = account("account", 0, SEED);

		Maintainer::<T>::put(maintainer.clone());

	}: _<T::Origin>(admin, maintainer.clone())
	verify {
		assert_last_event::<T>(Event::MaintainerSet{ old_maintainer: Some(maintainer), new_maintainer: new_maintainer }.into());
	}

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

		let mut resource_id = [0; 32];

		for i in 0..32 {
			resource_id[i] = i as u8;
		}

		let bytes = vec![0u8; c as usize];

	}: _<T::Origin>(admin, resource_id, bytes)
	verify {
		assert!(Pallet::<T>::resource_exists(resource_id) == true);
	}

	remove_resource {
		let c in 1 .. 16_000;

		let admin = T::AdminOrigin::successful_origin();

		let mut resource_id = [0; 32];

		for i in 0..32 {
			resource_id[i] = i as u8;
		}

		let bytes = vec![0u8; c as usize];

		Pallet::<T>::register_resource(resource_id, bytes);

	}: _<T::Origin>(admin, resource_id)
	verify {
		assert!(Pallet::<T>::resource_exists(resource_id) == false);
	}

	whitelist_chain {
		let admin = T::AdminOrigin::successful_origin();

		let chain_id: T::ChainId = CHAIN_IDENTIFIER.into();

	}: _<T::Origin>(admin, chain_id)
	verify {
		assert_last_event::<T>(Event::ChainWhitelisted{ chain_id: chain_id}.into());
	}

	add_proposer {
		let admin = T::AdminOrigin::successful_origin();

		let v: T::AccountId = account("account", 0, SEED);
	}: _<T::Origin>(admin, v.clone())
	verify {
		assert_last_event::<T>(Event::ProposerAdded{ proposer_id: v}.into());
	}

	remove_proposer {
		let admin = T::AdminOrigin::successful_origin();

		let v: T::AccountId = account("account", 0, SEED);

		crate::Pallet::<T>::register_proposer(v.clone());

	}: _<T::Origin>(admin, v.clone())
	verify {
		assert_last_event::<T>(Event::ProposerRemoved{ proposer_id: v}.into());
	}

	acknowledge_proposal {
		let c in 1 .. 16_000;

		let caller: T::AccountId = whitelisted_caller();

		let resource_id = [0; 32];

		let bytes = vec![0u8; 12];

		Pallet::<T>::register_resource(resource_id, bytes);

		let nonce = 1;

		let chain_id: T::ChainId = CHAIN_IDENTIFIER.into();

		let bytes = vec![0u8; c as usize];

		let proposal_bytes: T::Proposal = T::Proposal::decode(&mut &bytes[..]).unwrap();

		// updates proposal threshold to like 10
		// make like 9 accounts vote for the same proposal
		// make them proposers before they can vote
		// and then allow benchmark vote

	}: _(RawOrigin::Signed(caller.clone()), nonce, chain_id,  resource_id, proposal_bytes)
	verify {

	}
}
