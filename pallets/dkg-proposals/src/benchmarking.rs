//! Benchmarking for dkg-proposals
//!
use super::*;

#[allow(unused)]
use crate::Pallet;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller, account};
use frame_system::RawOrigin;

const SEED: u32 = 0;
const CHAIN_IDENTIFIER: u32 = 5;

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

benchmarks! {
	set_maintainer {
		let caller: T::AccountId = whitelisted_caller();

		let maintainer: T::AccountId = account("account", 0, SEED);

		let new_maintainer: T::AccountId  = account("account", 0, SEED);

		Maintainer::<T>::put(maintainer.clone());

	}: _(RawOrigin::Root, new_maintainer.clone())
	verify {
		assert_last_event::<T>(Event::MaintainerSet{ old_maintainer: Some(maintainer), new_maintainer: new_maintainer }.into());
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

	whitelist_chain {
		let admin = T::AdminOrigin::successful_origin();

		//let chain_id: T::ChainId = CHAIN_IDENTIFIER.into();

		//let chain_identifier: T::ChainIdentifier = chain_id.into();

		let chain_id: T::ChainId = T::ChainIdentifier::get();
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
}
