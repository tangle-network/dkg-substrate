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

use super::*;
use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{
	traits::Get, BoundedVec, CloneNoBound, DefaultNoBound, PartialEqNoBound, RuntimeDebugNoBound,
};
use pallet_identity::Data;
use scale_info::TypeInfo;
use sp_runtime::traits::AppendZerosInput;
use sp_std::prelude::*;

/// Information concerning the identity of the controller of an account.
///
/// NOTE: This should be stored at the end of the storage item to facilitate the addition of extra
/// fields in a backwards compatible way through a specialized `Decode` impl.
#[derive(
	DefaultNoBound,
	CloneNoBound,
	Encode,
	Decode,
	Eq,
	MaxEncodedLen,
	PartialEqNoBound,
	RuntimeDebugNoBound,
	TypeInfo,
)]
#[codec(mel_bound())]
#[scale_info(skip_type_params(FieldLimit))]
pub struct BridgeInfo<FieldLimit: Get<u32>> {
	/// Additional fields of the metadata that are not catered for with the struct's explicit
	/// fields.
	pub additional: BoundedVec<(Data, Data), FieldLimit>,

	/// A reasonable display name for the bridge. This should be whatever it is
	/// that it is typically known as and should not be confusable with other entities, given
	/// reasonable context.
	///
	/// Stored as UTF-8.
	pub display: Data,
}

/// Information concerning the identity of the controller of an account.
///
/// NOTE: This is stored separately primarily to facilitate the addition of extra fields in a
/// backwards compatible way through a specialized `Decode` impl.
#[derive(
	CloneNoBound, Encode, Eq, MaxEncodedLen, PartialEqNoBound, RuntimeDebugNoBound, TypeInfo,
)]
#[codec(mel_bound())]
#[scale_info(skip_type_params(MaxResources, MaxAdditionalFields))]
pub struct BridgeMetadata<MaxResources: Get<u32>, MaxAdditionalFields: Get<u32>> {
	/// A list of resource IDs for the bridge
	pub resource_ids: BoundedVec<ResourceId, MaxResources>,

	/// Auxilliary information on the bridge.
	pub info: BridgeInfo<MaxAdditionalFields>,
}

impl<MaxResources: Get<u32>, MaxAdditionalFields: Get<u32>> Decode
	for BridgeMetadata<MaxResources, MaxAdditionalFields>
{
	fn decode<I: codec::Input>(input: &mut I) -> sp_std::result::Result<Self, codec::Error> {
		let (resource_ids, info) = Decode::decode(&mut AppendZerosInput::new(input))?;
		Ok(Self { resource_ids, info })
	}
}

pub struct EvmProof {
	pub log_index: u64,
	pub log_entry_data: Vec<u8>,
	pub receipt_index: u64,
	pub receipt_data: Vec<u8>,
	pub header_data: Vec<u8>,
	pub proof: Vec<Vec<u8>>,
}
pub enum ProofData {
	EVM(EvmProof),
}

impl ProofData {
	pub fn to_evm_proof(&self) -> Option<&EvmProof> {
		match self {
			Self::EVM(evm_proof) => Some(evm_proof),
			_ => None,
		}
	}
}
