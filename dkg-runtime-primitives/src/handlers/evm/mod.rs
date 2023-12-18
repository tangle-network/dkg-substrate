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

/// Governor proposals
pub mod refresh;

/// EVM tx
pub mod evm_tx;

use webb_proposals::evm::*;

/// Macro for creating modules with proposal creation functions
macro_rules! proposal_module {
	($mod_name:ident, $proposal_type:ty, $proposal_length:expr) => {
		pub mod $mod_name {
			use super::*;
			use crate::handlers::validate_proposals::ValidationError;
			use codec::alloc::string::ToString;
			use webb_proposals::from_slice;

			pub fn create(data: &[u8]) -> Result<$proposal_type, ValidationError> {
				let bytes: [u8; $proposal_length] =
					data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
				from_slice::<$proposal_type>(&bytes).map_err(|e| {
					log::error!("Error while decoding proposal: {:?}", e);
					ValidationError::InvalidDecoding(e.to_string())
				})
			}
		}
	};
}

proposal_module!(add_token_to_set, TokenAddProposal, TokenAddProposal::LENGTH);
proposal_module!(remove_token_from_set, TokenRemoveProposal, TokenRemoveProposal::LENGTH);
proposal_module!(fee_update, WrappingFeeUpdateProposal, WrappingFeeUpdateProposal::LENGTH);
proposal_module!(
	fee_recipient_update,
	FeeRecipientUpdateProposal,
	FeeRecipientUpdateProposal::LENGTH
);
proposal_module!(
	max_deposit_limit_update,
	MaxDepositLimitProposal,
	MaxDepositLimitProposal::LENGTH
);
proposal_module!(
	min_withdrawal_limit_update,
	MinWithdrawalLimitProposal,
	MinWithdrawalLimitProposal::LENGTH
);
proposal_module!(rescue_tokens, RescueTokensProposal, RescueTokensProposal::LENGTH);
proposal_module!(anchor_update, AnchorUpdateProposal, AnchorUpdateProposal::LENGTH);
proposal_module!(resource_id_update, ResourceIdUpdateProposal, ResourceIdUpdateProposal::LENGTH);
proposal_module!(
	set_treasury_handler,
	SetTreasuryHandlerProposal,
	SetTreasuryHandlerProposal::LENGTH
);
proposal_module!(set_verifier, SetVerifierProposal, SetVerifierProposal::LENGTH);
