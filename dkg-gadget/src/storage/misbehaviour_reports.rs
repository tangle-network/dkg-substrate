use crate::{worker::DKGWorker, Client};
use codec::Encode;
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{
	crypto::AuthorityId, offchain::storage_keys::AGGREGATED_MISBEHAVIOUR_REPORTS,
	AggregatedMisbehaviourReports, DKGApi,
};
use log::trace;
use sc_client_api::Backend;
use sp_application_crypto::sp_core::offchain::{OffchainStorage, STORAGE_PREFIX};
use sp_runtime::traits::{Block, Header};

/// stores aggregated misbehaviour reports offchain
pub(crate) fn store_aggregated_misbehaviour_reports<B, C, BE>(
	dkg_worker: &mut DKGWorker<B, C, BE>,
	reports: &AggregatedMisbehaviourReports,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let maybe_offchain = dkg_worker.backend.offchain_storage();
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

	let _ = dkg_worker
		.aggregated_misbehaviour_reports
		.remove(&(reports.round_id, reports.offender.clone()));

	Ok(())
}
