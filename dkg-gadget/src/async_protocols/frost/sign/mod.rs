use crate::{
	async_protocols::{blockchain_interface::BlockchainInterface, types::VoteResult, BatchKey},
	dkg_modules::wt_frost::NetInterface,
};
use codec::Encode;
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{SessionId, StoredUnsignedProposalBatch};
use rand::thread_rng;
use std::{future::Future, pin::Pin};
use wsts::{
	common::PolyCommitment,
	v2::{Party, PartyState},
};

// TODO prototype testing: see if random selection of keygen parties works
pub fn protocol<Net: NetInterface, BI: BlockchainInterface>(
	t: u32,
	session_id: SessionId,
	net: Net,
	bc_iface: BI,
	public_key: Vec<PolyCommitment>,
	state: PartyState,
	unsigned_proposal_batch: StoredUnsignedProposalBatch<
		BI::BatchId,
		BI::MaxProposalLength,
		BI::MaxProposalsInBatch,
		BI::Clock,
	>,
) -> Pin<Box<dyn Future<Output = Result<(), DKGError>>>> {
	Box::pin(async move {
		let mut party = Party::load(&state);
		let mut rng = thread_rng();
		let k = state.num_keys;

		let batch_key = unsigned_proposal_batch.batch_id.clone();
		// The message we are signing is the encoded unsigned_proposal_batch
		let message = unsigned_proposal_batch.proposals.encode();

		let signature = crate::dkg_modules::wt_frost::run_signing(
			&mut party, &mut rng, &message, net, k, t, public_key,
		)
		.await?;
		bc_iface.process_vote_result(VoteResult::FROST {
			signature,
			unsigned_proposal_batch,
			session_id,
			batch_key,
			message,
		})?;
		Ok(())
	})
}
