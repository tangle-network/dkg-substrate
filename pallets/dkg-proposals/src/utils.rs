// This file is part of Webb.

// Copyright (C) 2021 Webb Technologies Inc.
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

//! # DKG Proposals Module
//! The pallet provides functionality for voting on what proposals should be signed by the DKG.
//!
//! ## Overview
//! The dkg-proposal pallet provides functions for:
//! - Setting proposers threshold
//! - Registering and unregistering resources
//! - Registering and unregistering proposers
//! - Whitelisting chains
//!
//! ### Terminology
//! - **Proposer**: A account that is trying to participate in the voting
//! - **Threshold**: The number of votes needed for a proposal to pass.
//!
//! ### Goals
//!
//! The DKG proposal system is designed to make the following
//! possible:
//!
//! * Allowing voting on what is going to be signed by the DKG.
//!
//! ## Interface
//!
//! ## Related Modules
//!
//! * [`System`](../frame_system/index.html)
//! * [`Support`](../frame_support/index.html)

use dkg_runtime_primitives::ResourceId;

/// Helper function to concatenate a chain ID and some bytes to produce a
/// resource ID. The common format is (26 bytes unique ID + 2 bytes chain type + 4 byte chain ID).
pub fn derive_resource_id(chain: u32, chain_type: u16, id: &[u8]) -> ResourceId {
	let mut r_id: ResourceId = [0; 32];
	let chain = chain.to_le_bytes();
	let chain_type = chain_type.to_le_bytes();
	r_id[26] = chain_type[0];
	r_id[27] = chain_type[1];
	// last 4 bytes of chain id
	r_id[28] = chain[0];
	r_id[29] = chain[1];
	r_id[30] = chain[2];
	r_id[31] = chain[3];
	let range = if id.len() > 26 { 26 } else { id.len() }; // Use at most 28 bytes
	for i in 0..range {
		r_id[25 - i] = id[range - 1 - i]; // Ensure left padding for eth compatibility
	}
	r_id
}
