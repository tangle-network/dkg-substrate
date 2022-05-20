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

use curv::elliptic::curves::Secp256k1;
use dkg_runtime_primitives::UnsignedProposal;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey, sign::CompletedOfflineStage,
};
use std::{
	fmt::{Debug, Formatter},
	sync::Arc,
};

pub mod blockchain_interface;
pub mod dkg_gossip_engine;
pub mod incoming;
pub mod meta_handler;
pub mod remote;
pub mod state_machine_interface;

#[derive(Clone)]
pub enum ProtocolType {
	Keygen {
		i: u16,
		t: u16,
		n: u16,
	},
	Offline {
		unsigned_proposal: Arc<UnsignedProposal>,
		i: u16,
		s_l: Vec<u16>,
		local_key: Arc<LocalKey<Secp256k1>>,
	},
	Voting {
		offline_stage: Arc<CompletedOfflineStage>,
		unsigned_proposal: Arc<UnsignedProposal>,
		i: u16,
	},
}

impl ProtocolType {
	pub fn get_i(&self) -> PartyIndex {
		match self {
			Self::Keygen { i, .. } | Self::Offline { i, .. } | Self::Voting { i, .. } => *i,
		}
	}

	pub fn get_unsigned_proposal(&self) -> Option<&UnsignedProposal> {
		match self {
			Self::Offline { unsigned_proposal, .. } | Self::Voting { unsigned_proposal, .. } =>
				Some(&*unsigned_proposal),
			_ => None,
		}
	}
}

impl Debug for ProtocolType {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			ProtocolType::Keygen { i, t, n } => {
				write!(f, "Keygen: (i, t, n) = ({}, {}, {})", i, t, n)
			},
			ProtocolType::Offline { i, unsigned_proposal, .. } => {
				write!(f, "Offline: (i, proposal) = ({}, {:?})", i, &unsigned_proposal.proposal)
			},
			ProtocolType::Voting { unsigned_proposal, .. } => {
				write!(f, "Voting: proposal = {:?}", &unsigned_proposal.proposal)
			},
		}
	}
}

pub type PartyIndex = u16;
pub type Threshold = u16;
pub type BatchId = u64;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct BatchKey {
	pub len: usize,
	pub id: BatchId,
}
