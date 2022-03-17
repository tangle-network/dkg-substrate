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

use crate::handlers::{
	decode_proposals::decode_proposal_header, validate_proposals::ValidationError,
};
use codec::alloc::string::ToString;

pub struct Bytes32UpdateProposal {
	pub header: webb_proposals::ProposalHeader,
	pub target: [u8; 32],
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     target              - 32 bytes  [40..72]
/// ]
/// Total Bytes: 32 + 4 + 4 + 32 = 72
pub fn create(data: &[u8]) -> Result<Bytes32UpdateProposal, ValidationError> {
	if data.len() != 72 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 72 bytes".to_string()))?
	}
	let header = decode_proposal_header(data)?;

	let mut target = [0u8; 32];
	target.copy_from_slice(&data[40..]);
	// TODO: Add validation over EVM address
	Ok(Bytes32UpdateProposal { header, target })
}
