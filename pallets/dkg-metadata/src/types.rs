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
use crate::*;
use codec::{Decode, Encode};
#[derive(Default, Encode, Decode, Clone, PartialEq, Eq, scale_info::TypeInfo)]
pub struct RoundMetadata<T: Config> {
	pub curr_round_pub_key: BoundedVec<u8, T::MaxKeyLength>,
	pub next_round_pub_key: BoundedVec<u8, T::MaxKeyLength>,
	pub refresh_signature: BoundedVec<u8, T::MaxSignatureLength>,
}
