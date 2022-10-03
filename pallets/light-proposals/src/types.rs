// This file is part of Substrate.

// Copyright (C) 2021-2022 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use codec::{Decode, Encode};
use frame_support::{CloneNoBound, PartialEqNoBound, RuntimeDebugNoBound};
use scale_info::TypeInfo;
use sp_std::prelude::*;

#[derive(CloneNoBound, Encode, Decode, Eq, PartialEqNoBound, RuntimeDebugNoBound, TypeInfo)]
pub struct EvmProof {
	pub log_index: u64,
	pub log_entry_data: Vec<u8>,
	pub receipt_index: u64,
	pub receipt_data: Vec<u8>,
	pub header_data: Vec<u8>,
	pub proof: Vec<Vec<u8>>,
}
#[derive(CloneNoBound, Encode, Decode, Eq, PartialEqNoBound, RuntimeDebugNoBound, TypeInfo)]
pub enum ProofData {
	EVM(EvmProof),
}

impl ProofData {
	#[allow(unreachable_patterns)]
	pub fn to_evm_proof(&self) -> Option<&EvmProof> {
		match self {
			ProofData::EVM(evm_proof) => Some(evm_proof),
			_ => None,
		}
	}
}
