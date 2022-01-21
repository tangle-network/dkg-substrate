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

use codec::{Decode, Encode};
use dkg_runtime_primitives::{
	mmr::{DKGNextAuthoritySet, MmrLeafVersion},
	AuthoritySet,
};
use hex_literal::hex;

use sp_core::H256;
use sp_io::TestExternalities;
use sp_runtime::{traits::Keccak256, DigestItem};

use frame_support::traits::OnInitialize;

use crate::mock::*;

fn init_block(block: u64) {
	System::set_block_number(block);
	Session::on_initialize(block);
	MMR::on_initialize(block);
	DKG::on_initialize(block);
	DKGMmr::on_initialize(block);
}

pub fn dkg_log(log: ConsensusLog<DKGId>) -> DigestItem {
	DigestItem::Consensus(DKG_ENGINE_ID, log.encode())
}

fn offchain_key(pos: usize) -> Vec<u8> {
	(<Test as pallet_mmr::Config>::INDEXING_PREFIX, pos as u64).encode()
}

fn read_mmr_leaf(ext: &mut TestExternalities, index: usize) -> MmrLeaf {
	type Node = pallet_mmr_primitives::DataOrHash<Keccak256, MmrLeaf>;
	ext.persist_offchain_overlay();
	let offchain_db = ext.offchain_db();
	offchain_db
		.get(&offchain_key(index))
		.map(|d| Node::decode(&mut &*d).unwrap())
		.map(|n| match n {
			Node::Data(d) => d,
			_ => panic!("Unexpected MMR node."),
		})
		.unwrap()
}
