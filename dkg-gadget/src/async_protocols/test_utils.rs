#![allow(clippy::unwrap_used)] // allow unwraps in tests
use crate::{
	async_protocols::{blockchain_interface::BlockchainInterface, BatchKey},
	proposal::make_signed_proposal,
};
use codec::Encode;
use curv::{elliptic::curves::Secp256k1, BigInt};
use dkg_primitives::{
	types::{DKGError, DKGMessage, SessionId, SignedDKGMessage},
	utils::convert_signature,
};
use dkg_runtime_primitives::{
	crypto::Public,
	gossip_messages::{DKGSignedPayload, PublicKeyMessage},
	MaxProposalLength, UnsignedProposal,
};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::SignatureRecid, state_machine::keygen::LocalKey,
};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use webb_proposals::{Proposal, ProposalKind};

use super::KeygenPartyId;

pub(crate) type VoteResults =
	Arc<Mutex<HashMap<BatchKey, Vec<(Proposal<MaxProposalLength>, SignatureRecid, BigInt)>>>>;

#[derive(Clone)]
pub struct TestDummyIface {
	pub sender: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<Public>>,
	pub best_authorities: Arc<Vec<(KeygenPartyId, Public)>>,
	pub authority_public_key: Arc<Public>,
	// key is party_index, hash of data. Needed especially for local unit tests
	pub vote_results: VoteResults,
	pub keygen_key: Arc<Mutex<Option<LocalKey<Secp256k1>>>>,
}

#[async_trait::async_trait]
impl BlockchainInterface for TestDummyIface {
	type Clock = u32;
	type GossipEngine = ();
	type MaxProposalLength = MaxProposalLength;

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

	fn process_vote_result(
		&self,
		signature_rec: SignatureRecid,
		unsigned_proposal: UnsignedProposal<MaxProposalLength>,
		session_id: SessionId,
		batch_key: BatchKey,
		message: BigInt,
	) -> Result<(), DKGError> {
		let mut lock = self.vote_results.lock();
		let _payload_key = unsigned_proposal.key;
		let signature = convert_signature(&signature_rec).ok_or_else(|| {
			DKGError::CriticalError { reason: "Unable to serialize signature".to_string() }
		})?;

		let finished_round = DKGSignedPayload {
			key: session_id.encode(),
			payload: "Webb".encode(),
			signature: signature.encode(),
		};

		let prop = make_signed_proposal(ProposalKind::EVM, finished_round).unwrap();
		lock.entry(batch_key).or_default().push((prop.unwrap(), signature_rec, message));

		Ok(())
	}

	fn gossip_public_key(&self, _key: PublicKeyMessage) -> Result<(), DKGError> {
		// we do not gossip the public key in the test interface
		Ok(())
	}

	fn store_public_key(&self, key: LocalKey<Secp256k1>, _: SessionId) -> Result<(), DKGError> {
		*self.keygen_key.lock() = Some(key);
		Ok(())
	}

	fn get_authority_set(&self) -> Vec<(KeygenPartyId, Public)> {
		(*self.best_authorities).clone()
	}

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine> {
		None
	}

	fn now(&self) -> Self::Clock {
		0
	}
}
