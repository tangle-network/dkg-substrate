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

use webb_proposals::evm::TokenRemoveProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<TokenRemoveProposal, ValidationError> {
	let bytes: [u8; TokenRemoveProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(TokenRemoveProposal::from(bytes))
}
