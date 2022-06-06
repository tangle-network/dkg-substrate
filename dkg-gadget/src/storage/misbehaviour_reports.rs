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
use crate::{meta_async_rounds::dkg_gossip_engine::GossipEngineIface, worker::DKGWorker, Client};
use codec::Encode;
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::AGGREGATED_MISBEHAVIOUR_REPORTS,
	AggregatedMisbehaviourReports, DKGApi,
};
use log::trace;
use sc_client_api::Backend;
use sp_application_crypto::sp_core::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::traits::{Block, NumberFor};
use crate::worker::{HasAggregatedMisbehaviourReports, HasBackend};

/// stores aggregated misbehaviour reports offchain
pub(crate) fn store_aggregated_misbehaviour_reports<B: Block, BE: Backend<B>, BCE: HasBackend<B, BE> + HasAggregatedMisbehaviourReports>(
	bc_iface: BCE,
	reports: &AggregatedMisbehaviourReports<AuthorityId>,
) -> Result<(), DKGError>
{
	let maybe_offchain = bc_iface.get_backend().offchain_storage();
	if maybe_offchain.is_none() {
		return Err(DKGError::GenericError { reason: "No offchain storage available".to_string() })
	}

	let mut offchain = maybe_offchain.unwrap();
	offchain.set(STORAGE_PREFIX, AGGREGATED_MISBEHAVIOUR_REPORTS, &reports.clone().encode());
	trace!(
		target: "dkg",
		"Stored aggregated misbehaviour reports {:?}",
		reports.encode()
	);

	let _ = bc_iface.get_aggregated_misbehaviour_reports().lock().remove(&(
		reports.misbehaviour_type,
		reports.round_id,
		reports.offender.clone(),
	));

	Ok(())
}
