#![allow(clippy::unwrap_used)] // allow unwraps in tests
use crate::{
	async_protocols::{
		blockchain_interface::BlockchainInterface,
		types::{LocalKeyType, VoteResult},
		BatchKey,
	},
	db::DKGDbBackend,
};
use codec::Encode;
use dkg_primitives::types::{DKGError, DKGMessage, SessionId, SignedDKGMessage};
use dkg_runtime_primitives::{
	crypto::Public, gossip_messages::PublicKeyMessage, BatchId, MaxProposalLength,
	MaxProposalsInBatch, MaxSignatureLength, SignedProposalBatch,
};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use webb_proposals::Proposal;

use super::KeygenPartyId;

pub(crate) type VoteResults = Arc<
	Mutex<
		HashMap<
			BatchKey,
			Vec<
				SignedProposalBatch<
					BatchId,
					MaxProposalLength,
					MaxProposalsInBatch,
					MaxSignatureLength,
				>,
			>,
		>,
	>,
>;

#[derive(Clone)]
pub struct TestDummyIface {
	pub sender: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<Public>>,
	pub best_authorities: Arc<Vec<(KeygenPartyId, Public)>>,
	pub authority_public_key: Arc<Public>,
	// key is party_index, hash of data. Needed especially for local unit tests
	pub vote_results: VoteResults,
	pub keygen_key: Arc<Mutex<Option<LocalKeyType>>>,
}

#[async_trait::async_trait]
impl BlockchainInterface for TestDummyIface {
	type Clock = u32;
	type GossipEngine = ();
	type MaxProposalLength = MaxProposalLength;
	type BatchId = BatchId;
	type MaxProposalsInBatch = MaxProposalsInBatch;
	type MaxSignatureLength = MaxSignatureLength;

	async fn verify_signature_against_authorities(
		&self,
		message: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError> {
		Ok(message.msg)
	}

	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
		dkg_logging::info!(
			"Sending message through iface id={}",
			unsigned_msg
				.payload
				.async_proto_only_get_sender_id()
				.expect("Could not get sender id")
		);
		let faux_signed_message = SignedDKGMessage { msg: unsigned_msg, signature: None };
		self.sender
			.send(faux_signed_message)
			.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
		Ok(())
	}

	fn process_vote_result(&self, result: VoteResult<Self>) -> Result<(), DKGError> {
		if let VoteResult::ECDSA {
			signature,
			unsigned_proposal_batch,
			session_id: _sid,
			batch_key,
		} = result
		{
			let mut lock = self.vote_results.lock();

			let mut signed_proposals = vec![];

			// convert all unsigned proposals to signed
			for unsigned_proposal in unsigned_proposal_batch.proposals.iter() {
				signed_proposals.push(Proposal::Signed {
					kind: unsigned_proposal.proposal.kind(),
					data: unsigned_proposal
						.data()
						.clone()
						.try_into()
						.expect("should not happen since its a valid proposal"),
					signature: signature
						.encode()
						.try_into()
						.expect("Signature exceeds runtime bounds!"),
				});
			}

			let signed_proposal_batch = SignedProposalBatch {
				batch_id: unsigned_proposal_batch.batch_id,
				proposals: signed_proposals.try_into().expect("Proposals exceeds runtime bounds!"),
				signature: signature
					.encode()
					.try_into()
					.expect("Signature exceeds runtime bounds!"),
			};

			let proposals_for_this_batch = lock.entry(batch_key).or_default();
			proposals_for_this_batch.push(signed_proposal_batch);

			Ok(())
		} else {
			panic!("Only ECDSA is supported in the test interface");
		}
	}

	fn gossip_public_key(&self, _key: PublicKeyMessage) -> Result<(), DKGError> {
		// we do not gossip the public key in the test interface
		Ok(())
	}

	fn store_public_key(&self, key: LocalKeyType, _: SessionId) -> Result<(), DKGError> {
		*self.keygen_key.lock() = Some(key);
		Ok(())
	}

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine> {
		None
	}

	fn get_backend_db(&self) -> Option<&Arc<dyn DKGDbBackend>> {
		None
	}

	fn now(&self) -> Self::Clock {
		0
	}
}
