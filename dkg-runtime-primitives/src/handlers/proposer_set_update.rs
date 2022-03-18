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

use crate::{
	handlers::{validate_proposals::ValidationError}, ProposalNonce, Vec,
};
use codec::alloc::string::ToString;

pub struct ProposerSetUpdateProposal {
	pub merkle_root: Vec<u8>,        // 32 bytes
	pub average_session_length: u64, // 8 bytes
	pub num_of_proposers: u32,       // 4 bytes
	pub nonce: ProposalNonce,        // 4 bytes
}

/// Proposal Data: [
///     merkle_root: 32 bytes
///     average_session_length: 8 bytes
/// 	num_of_proposers: 4 bytes
///     nonce: 4 bytes
/// ]
/// Total Bytes: 32 + 8 + 4 + 4= 48 bytes
pub fn create(data: &[u8]) -> Result<ProposerSetUpdateProposal, ValidationError> {
	if data.len() != 48 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 50 bytes".to_string()))?
	}

	let mut merkle_root_bytes = [0u8; 32];
	merkle_root_bytes.copy_from_slice(&data[0..32]);
	let merkle_root = merkle_root_bytes.to_vec();

	let mut average_session_length_bytes = [0u8; 8];
	average_session_length_bytes.copy_from_slice(&data[32..40]);
	let average_session_length = u64::from_be_bytes(average_session_length_bytes);

	let mut num_of_proposers_bytes = [0u8; 4];
	num_of_proposers_bytes.copy_from_slice(&data[40..44]);
	let num_of_proposers = u32::from_be_bytes(num_of_proposers_bytes);

	let mut nonce_bytes = [0u8; 4];
	nonce_bytes.copy_from_slice(&data[44..48]);
	let nonce = u32::from_be_bytes(nonce_bytes).into();

	Ok(ProposerSetUpdateProposal { merkle_root, average_session_length, num_of_proposers, nonce })
}
