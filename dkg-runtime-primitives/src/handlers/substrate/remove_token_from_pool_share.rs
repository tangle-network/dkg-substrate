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
use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	Vec,
};
use codec::alloc::string::ToString;

pub struct RemoveTokenProposal {
	pub header: webb_proposals::ProposalHeader,
	pub encoded_call: Vec<u8>,
}

/// https://github.com/webb-tools/protocol-substrate/issues/142
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     zeroes              - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     call                -          [40..]
/// ]
/// Total Bytes: 32 + 4 + 4 + (size of call) = 40 + (size of call)
pub fn create(data: &[u8]) -> Result<RemoveTokenProposal, ValidationError> {
	let header = decode_proposal_header(data)?;
	let zeroes = header.function_signature().to_bytes();
	// Check that zeroes is actually zero
	if u32::from_be_bytes(zeroes) != 0 {
		return Err(ValidationError::InvalidParameter("Function Sig should be zero".to_string()))
	}
	let encoded_call = data[40..].to_vec();
	Ok(RemoveTokenProposal { header, encoded_call })
}
