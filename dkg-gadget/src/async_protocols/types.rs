use crate::async_protocols::{blockchain_interface::BlockchainInterface, BatchKey};
use curv::{elliptic::curves::Secp256k1, BigInt};
use dkg_runtime_primitives::{SessionId, StoredUnsignedProposalBatch};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::SignatureRecid, state_machine::keygen::LocalKey,
};
use serde::{Deserialize, Serialize};
use wsts::{
	common::{PolyCommitment, Signature},
	v2::PartyState,
};

#[derive(Serialize, Deserialize, Clone)]
pub enum LocalKeyType {
	ECDSA(LocalKey<Secp256k1>),
	FROST(Vec<PolyCommitment>, PartyState),
}

pub enum VoteResult<BI: BlockchainInterface> {
	ECDSA {
		signature: SignatureRecid,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BI::BatchId,
			BI::MaxProposalLength,
			BI::MaxProposalsInBatch,
			BI::Clock,
		>,
		session_id: SessionId,
		batch_key: BatchKey,
		message: BigInt,
	},
	FROST {
		signature: Signature,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BI::BatchId,
			BI::MaxProposalLength,
			BI::MaxProposalsInBatch,
			BI::Clock,
		>,
		session_id: SessionId,
		batch_key: BatchKey,
		message: Vec<u8>,
	},
}
