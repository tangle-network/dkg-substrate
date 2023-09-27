use crate::{
	async_protocols::{blockchain_interface::BlockchainInterface, types::VoteResult, BatchKey},
	dkg_modules::wt_frost::NetInterface,
	utils::SendFuture,
};
use codec::Encode;
use dkg_runtime_primitives::{SessionId, StoredUnsignedProposalBatch};
use std::pin::Pin;
use wsts::{
	common::PolyCommitment,
	v2::{Party, PartyState},
};

// TODO prototype testing: see if random selection of keygen parties works
#[allow(clippy::too_many_arguments)]
pub fn protocol<Net: NetInterface + 'static, BI: BlockchainInterface + 'static>(
	t: u32,
	session_id: SessionId,
	batch_key: BatchKey,
	mut net: Net,
	bc_iface: BI,
	public_key: Vec<PolyCommitment>,
	state: PartyState,
	unsigned_proposal_batch: StoredUnsignedProposalBatch<
		BI::BatchId,
		BI::MaxProposalLength,
		BI::MaxProposalsInBatch,
		BI::Clock,
	>,
) -> Pin<Box<dyn SendFuture<'static, ()>>> {
	Box::pin(async move {
		let mut party = Party::load(&state);
		let mut rng = rand::rngs::OsRng;
		let k = state.num_keys;

		// The message we are signing is the encoded unsigned_proposal_batch
		let message = unsigned_proposal_batch.proposals.encode();

		let signature = crate::dkg_modules::wt_frost::run_signing(
			&mut party, &mut rng, &message, &mut net, k, t, public_key,
		)
		.await?;
		bc_iface.process_vote_result(VoteResult::FROST {
			signature: signature.into(),
			unsigned_proposal_batch,
			session_id,
			batch_key,
		})?;
		Ok(())
	})
}
