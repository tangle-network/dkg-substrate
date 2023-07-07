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

/// Bridge proposals
pub mod refresh;
pub mod resource_id_update;

/// Anchor proposals
pub mod anchor_update;
pub mod set_verifier;

/// Token update proposals
pub mod add_token_to_set;
pub mod fee_update;
pub mod remove_token_from_set;

/// Treasury Proposals
pub mod fee_recipient_update;
pub mod set_treasury_handler;

/// Rescue tokens
pub mod rescue_tokens;

/// fees & limits
pub mod max_deposit_limit_update;
pub mod min_withdrawal_limit_update;

/// EVM tx
pub mod evm_tx;
