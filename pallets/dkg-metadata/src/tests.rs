// Copyright (C) 2020 - 2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::vec;

use codec::Encode;
use dkg_runtime_primitives::AuthoritySet;

use sp_core::H256;
use sp_runtime::DigestItem;

use frame_support::traits::OnInitialize;

use crate::mock::*;

fn init_block(block: u64) {
	System::set_block_number(block);
	Session::on_initialize(block);
}

pub fn dkg_log(log: ConsensusLog<DKGId>) -> DigestItem<H256> {
	DigestItem::Consensus(DKG_ENGINE_ID, log.encode())
}

#[test]
fn genesis_session_initializes_authorities() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		let authorities = DKGMetadata::authorities();

		assert!(authorities.len() == 2);
		assert_eq!(want[0], authorities[0]);
		assert_eq!(want[1], authorities[1]);

		assert!(DKGMetadata::authority_set_id() == 0);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[0], next_authorities[0]);
		assert_eq!(want[1], next_authorities[1]);
	});
}

#[test]
fn session_change_updates_authorities() {
	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);

		assert!(0 == DKGMetadata::authority_set_id());

		// no change - no log
		assert!(System::digest().logs.is_empty());

		init_block(2);

		assert!(1 == DKGMetadata::authority_set_id());

		let want = dkg_log(ConsensusLog::AuthoritiesChange {
			next_authorities: AuthoritySet {
				authorities: vec![mock_dkg_id(3), mock_dkg_id(4)],
				id: 1,
			},
			next_queued_authorities: AuthoritySet {
				authorities: vec![mock_dkg_id(3), mock_dkg_id(4)],
				id: 2,
			},
		});

		let log = System::digest().logs[0].clone();

		assert_eq!(want, log);
	});
}

#[test]
fn session_change_updates_next_authorities() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[0], next_authorities[0]);
		assert_eq!(want[1], next_authorities[1]);

		init_block(2);

		let next_authorities = DKGMetadata::next_authorities();

		assert!(next_authorities.len() == 2);
		assert_eq!(want[2], next_authorities[0]);
		assert_eq!(want[3], next_authorities[1]);
	});
}

#[test]
fn authority_set_at_genesis() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 0u64);
		assert_eq!(vs.authorities[0], want[0]);
		assert_eq!(vs.authorities[1], want[1]);
	});
}

#[test]
fn authority_set_updates_work() {
	let want = vec![mock_dkg_id(1), mock_dkg_id(2), mock_dkg_id(3), mock_dkg_id(4)];

	new_test_ext(vec![1, 2, 3, 4]).execute_with(|| {
		init_block(1);

		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 0u64);
		assert_eq!(want[0], vs.authorities[0]);
		assert_eq!(want[1], vs.authorities[1]);

		init_block(2);

		let vs = DKGMetadata::authority_set();

		assert_eq!(vs.id, 1u64);
		assert_eq!(want[2], vs.authorities[0]);
		assert_eq!(want[3], vs.authorities[1]);
	});
}
