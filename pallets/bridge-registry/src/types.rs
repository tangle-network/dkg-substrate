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
	serde::Serialize,
	serde::Deserialize,
)]
#[codec(mel_bound())]
#[scale_info(skip_type_params(FieldLimit))]
pub struct BridgeInfo<FieldLimit: Get<u32>> {
	/// Additional fields of the metadata that are not catered for with the struct's explicit
	/// fields.
	#[cfg_attr(feature = "std", serde(bound = ""))]
	pub additional: BoundedVec<(SerdeData, SerdeData), FieldLimit>,

	/// A reasonable display name for the bridge. This should be whatever it is
	/// that it is typically known as and should not be confusable with other entities, given
	/// reasonable context.
	///
	/// Stored as UTF-8.
	pub display: SerdeData,
}

#[derive(
	Clone, Eq, PartialEq, sp_runtime::RuntimeDebug, MaxEncodedLen, Encode, TypeInfo, Decode, Default,
)]
pub struct SerdeData(Data);

mod serde_ {
	use super::*;
	use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
	struct DataVisitor;
	use sp_std::{fmt::Formatter, str::FromStr};

	impl FromStr for SerdeData {
		type Err = ();

		fn from_str(s: &str) -> Result<Self, Self::Err> {
			let raw_data = s.as_bytes().to_vec().try_into().map_err(|_| ())?;
			Ok(SerdeData(Data::Raw(raw_data)))
		}
	}

	impl<'de> Visitor<'de> for DataVisitor {
		type Value = SerdeData;

		fn expecting(&self, formatter: &mut Formatter) -> core::fmt::Result {
			formatter.write_str("a hex string with at most 32 bytes")
		}

		fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
		where
			E: serde::de::Error,
		{
			use serde::de::{Error, Unexpected};
			let v_hex_decoded =
				hex::decode(v).map_err(|_| Error::invalid_value(Unexpected::Str(v), &self))?;
			let v_bounded = v_hex_decoded
				.try_into()
				.map_err(|_| Error::invalid_value(Unexpected::Str(v), &self))?;
			Ok(SerdeData(Data::Raw(v_bounded)))
		}
	}
	impl<'de> Deserialize<'de> for SerdeData {
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where
			D: Deserializer<'de>,
		{
			deserializer.deserialize_str(DataVisitor)
		}
	}

	impl Serialize for SerdeData {
		fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where
			S: Serializer,
		{
			serializer.serialize_str(&hex::encode(self.0.encode()))
		}
	}
}

/// Information concerning the identity of the controller of an account.
///
/// NOTE: This is stored separately primarily to facilitate the addition of extra fields in a
/// backwards compatible way through a specialized `Decode` impl.
#[derive(
	CloneNoBound,
	Encode,
	Eq,
	MaxEncodedLen,
	PartialEqNoBound,
	RuntimeDebugNoBound,
	TypeInfo,
	serde::Serialize,
	serde::Deserialize,
)]
#[codec(mel_bound())]
#[scale_info(skip_type_params(MaxResources, MaxAdditionalFields))]
pub struct BridgeMetadata<MaxResources: Get<u32>, MaxAdditionalFields: Get<u32>> {
	/// A list of resource IDs for the bridge
	#[cfg_attr(feature = "std", serde(bound = ""))]
	pub resource_ids: BoundedVec<ResourceId, MaxResources>,

	/// Auxilliary information on the bridge.
	#[cfg_attr(feature = "std", serde(bound = ""))]
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
