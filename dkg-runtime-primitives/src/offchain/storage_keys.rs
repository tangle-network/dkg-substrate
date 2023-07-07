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
// Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_PUBLIC_KEYS: &[u8] = b"dkg-metadata::public_key";

// Lock Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_PUBLIC_KEYS_LOCK: &[u8] = b"dkg-metadata::public_key::lock";

// Key for offchain storage of aggregated derived public keys for genesis authorities
pub const AGGREGATED_PUBLIC_KEYS_AT_GENESIS: &[u8] = b"dkg-metadata::genesis_public_keys";

// Lock Key for offchain storage of aggregated derived public keys for genesis authorities
pub const AGGREGATED_PUBLIC_KEYS_AT_GENESIS_LOCK: &[u8] =
	b"dkg-metadata::genesis_public_keys::lock";

// Key for offchain storage of derived public key
pub const SUBMIT_KEYS_AT: &[u8] = b"dkg-metadata::submit_keys_at";

// Key for offchain storage of derived public key
pub const SUBMIT_GENESIS_KEYS_AT: &[u8] = b"dkg-metadata::submit_genesis_keys_at";

// Key for offchain storage of derived public key signature
pub const OFFCHAIN_PUBLIC_KEY_SIG: &[u8] = b"dkg-metadata::public_key_sig";

// Lock Key for offchain storage of derived public key signature
pub const OFFCHAIN_PUBLIC_KEY_SIG_LOCK: &[u8] = b"dkg-metadata::public_key_sig::lock";

// Key for offchain signed proposals storage
pub const OFFCHAIN_SIGNED_PROPOSALS: &[u8] = b"dkg-proposal-handler::signed_proposals";

// Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_MISBEHAVIOUR_REPORTS: &[u8] = b"dkg-metadata::misbehaviour";

// Lock Key for offchain storage of aggregated derived public keys
pub const AGGREGATED_MISBEHAVIOUR_REPORTS_LOCK: &[u8] = b"dkg-metadata::misbehaviour::lock";

// Key for offchain storage of aggregated proposer votes
pub const AGGREGATED_PROPOSER_VOTES: &[u8] = b"dkg-metadata::proposer_votes";

// Lock Key for offchain storage of aggregated proposer votes
pub const AGGREGATED_PROPOSER_VOTES_LOCK: &[u8] = b"dkg-metadata::proposer_votes::lock";

// Lock Key for submitting signed proposal on chain
pub const SUBMIT_SIGNED_PROPOSAL_ON_CHAIN_LOCK: &[u8] =
	b"dkg-proposal-handler::submit_signed_proposal_on_chain::lock";
