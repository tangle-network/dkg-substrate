//! Benchmarking for dkg-proposals
//!
use super::*;

#[allow(unused)]
use crate::Pallet;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::RawOrigin;
use sp_core::U256;

const SEED: u32 = 0;

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

benchmarks! {
	set_maintainer {
		let admin: T::AccountId = T::AdminOrigin::successful_origin();

		let maintainer: T::AccountId = account("account", 0, SEED);
	}: _(admin, maintainer)
	verify {
		assert_last_event::<T>(Event::MaintainerSet{ old_maintainer: admin, new_maintainer: maintainer }.into());
	}

	force_set_maintainer {
		let admin: T::AccountId = T::AdminOrigin::successful_origin();

		let maintainer: T::AccountId = account("account", 0, SEED);
	}: _(admin, maintainer)
	verify {
		assert_last_event::<T>(Event::MaintainerSet{ old_maintainer: admin, new_maintainer: maintainer }.into());
	}

	set_threshold {
		let c in 0 .. 16_000;

		let admin: T::AccountId = T::AdminOrigin::successful_origin();

	}: _(admin, c as u32)
	verify {
		assert_last_event::<T>(Event::ProposerThresholdChanged { new_threshold: c}.into());
	}

	add_proposer {
		let admin: T::AccountId = T::AdminOrigin::successful_origin();

		let v: T::AccountId = account("account", 0, SEED);
	}: _(admin, v)
	verify {
		assert_last_event::<T>(Event::ProposerAdded{ proposer_id: v}.into());
	}

	remove_proposer {
		let admin: T::AccountId = T::AdminOrigin::successful_origin();

		let v: T::AccountId = account("account", 0, SEED);
	}: _(admin, v)
	verify {
		assert_last_event::<T>(Event::ProposerRemoved{ proposer_id: v}.into());
	}
}
