use crate::async_protocols::{blockchain_interface::BlockchainInterface, BatchKey};
use codec::Encode;
use curv::elliptic::curves::Secp256k1;
use dkg_runtime_primitives::{SessionId, StoredUnsignedProposalBatch};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use wsts::{
	common::{PolyCommitment, Signature},
	v2::PartyState,
	Point, Scalar,
};

#[derive(Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum LocalKeyType {
	ECDSA(LocalKey<Secp256k1>),
	FROST(Vec<PolyCommitment>, PartyState),
}

impl Debug for LocalKeyType {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			LocalKeyType::ECDSA(key) => write!(f, "ECDSA({key:?})"),
			LocalKeyType::FROST(key, ..) => write!(f, "FROST({key:?})"),
		}
	}
}

impl Clone for LocalKeyType {
	fn clone(&self) -> Self {
		match self {
			LocalKeyType::ECDSA(key) => LocalKeyType::ECDSA(key.clone()),
			LocalKeyType::FROST(key, state) => {
				// PartyState does not impl Clone, but it does impl Serialize and Deserialize
				let state = bincode2::serialize(state).expect("Failed to serialize state");
				let state = bincode2::deserialize(&state).expect("Failed to deserialize state");
				LocalKeyType::FROST(key.clone(), state)
			},
		}
	}
}

pub enum VoteResult<BI: BlockchainInterface> {
	ECDSA {
		signature: sp_core::ecdsa::Signature,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BI::BatchId,
			BI::MaxProposalLength,
			BI::MaxProposalsInBatch,
			BI::Clock,
		>,
		session_id: SessionId,
		batch_key: BatchKey,
	},
	FROST {
		signature: FrostSignature,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BI::BatchId,
			BI::MaxProposalLength,
			BI::MaxProposalsInBatch,
			BI::Clock,
		>,
		session_id: SessionId,
		batch_key: BatchKey,
	},
}

#[derive(Encode)]
pub struct FrostSignature {
	pub inner: sp_core::Bytes,
}

impl From<Signature> for FrostSignature {
	fn from(inner: Signature) -> Self {
		let intermediate = Intermediate { point: inner.R, scalar: inner.z };

		let serialized_bytes =
			bincode2::serialize(&intermediate).expect("Failed to serialize signature");

		Self { inner: sp_core::Bytes::from(serialized_bytes) }
	}
}

#[derive(Serialize, Deserialize)]
struct Intermediate {
	point: Point,
	scalar: Scalar,
}
