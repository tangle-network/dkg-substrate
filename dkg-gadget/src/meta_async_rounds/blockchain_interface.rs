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

use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use codec::Encode;
use curv::BigInt;
use curv::elliptic::curves::Secp256k1;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::SignatureRecid;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
use parking_lot::{Mutex, RwLock};
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use crate::{Client, DKGApi};
use sc_network_gossip::GossipEngine;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_runtime::generic::BlockId;
use sp_runtime::traits::{Block, Header, NumberFor};
use dkg_primitives::types::{DKGError, DKGMessage, DKGPublicKeyMessage, DKGSignedPayload, RoundId, SignedDKGMessage};
use dkg_primitives::utils::convert_signature;
use dkg_runtime_primitives::crypto::{AuthorityId, Public};
use dkg_runtime_primitives::{AggregatedPublicKeys, AuthoritySet, Proposal, ProposalKind, UnsignedProposal};
use crate::DKGKeystore;
use crate::messages::dkg_message::sign_and_send_messages;
use crate::messages::public_key_gossip::gossip_public_key;
use crate::meta_async_rounds::BatchKey;
use crate::persistence::store_localkey;
use crate::proposal::{get_signed_proposal, make_signed_proposal};
use crate::storage::proposals::save_signed_proposals_in_storage;
use crate::worker::{DKGWorker, KeystoreExt};

pub trait BlockChainIface: Send + Sync {
	type Clock: AtLeast32BitUnsigned + Copy + Send + Sync;
	fn verify_signature_against_authorities(
		&self,
		message: Arc<SignedDKGMessage<Public>>,
	) -> Result<DKGMessage<Public>, DKGError>;
	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError>;
	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal: UnsignedProposal,
		round_id: RoundId,
		batch_key: BatchKey,
		message: BigInt,
	) -> Result<(), DKGError>;
	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError>;
	fn store_public_key(
		&self,
		key: LocalKey<Secp256k1>,
		round_id: RoundId,
	) -> Result<(), DKGError>;
	fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError>;
	fn get_authority_set(&self) -> &Vec<Public>;
	/// Get the unjailed signers
	fn get_unjailed_signers(&self) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner()?;
		Ok(self
			.get_authority_set()
			.iter()
			.enumerate()
			.filter(|(_, key)| !jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}

	/// Get the jailed signers
	fn get_jailed_signers(&self) -> Result<Vec<u16>, DKGError> {
		let jailed_signers = self.get_jailed_signers_inner()?;
		Ok(self
			.get_authority_set()
			.iter()
			.enumerate()
			.filter(|(_, key)| jailed_signers.contains(key))
			.map(|(i, _)| u16::try_from(i + 1).unwrap_or_default())
			.collect())
	}
}

pub struct DKGIface<B: Block, BE, C> {
	pub backend: Arc<BE>,
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
	pub client: Arc<C>,
	pub keystore: DKGKeystore,
	pub gossip_engine: Arc<Mutex<GossipEngine<B>>>,
	pub aggregated_public_keys: Arc<Mutex<HashMap<RoundId, AggregatedPublicKeys>>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	pub vote_results: Arc<Mutex<HashMap<BatchKey, Vec<Proposal>>>>,
	pub is_genesis: bool,
	pub _pd: PhantomData<BE>,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub local_keystore: Option<Arc<LocalKeystore>>,
	pub local_key_path: Option<PathBuf>,
}

impl<B, BE, C> BlockChainIface for DKGIface<B, BE, C>
	where
		B: Block,
		BE: Backend<B> + 'static,
		C: Client<B, BE> + 'static,
		C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	type Clock = NumberFor<B>;

	fn verify_signature_against_authorities(
		&self,
		msg: Arc<SignedDKGMessage<Public>>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let client = &self.client;

		DKGWorker::verify_signature_against_authorities_inner(
			(&*msg).clone(),
			&self.latest_header,
			client,
		)
	}

	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
		sign_and_send_messages(&self.gossip_engine, &self.keystore, unsigned_msg);
		Ok(())
	}

	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal: UnsignedProposal,
		round_id: RoundId,
		batch_key: BatchKey,
		_message: BigInt,
	) -> Result<(), DKGError> {
		// Call worker.rs: handle_finished_round -> Proposal
		// aggregate Proposal into Vec<Proposal>
		let payload_key = unsigned_proposal.key;
		let signature = convert_signature(&signature).ok_or_else(|| {
			DKGError::CriticalError { reason: "Unable to serialize signature".to_string() }
		})?;

		let finished_round = DKGSignedPayload {
			key: round_id.encode(),
			payload: unsigned_proposal.data().clone(),
			signature: signature.encode(),
		};

		let mut lock = self.vote_results.lock();
		let proposals_for_this_batch = lock.entry(batch_key).or_default();

		if let Some(proposal) =
		get_signed_proposal::<B, C, BE>(&self.backend, finished_round, payload_key)
		{
			proposals_for_this_batch.push(proposal);

			if proposals_for_this_batch.len() == batch_key.len {
				log::info!(target: "dkg", "All proposals have resolved for batch {:?}", batch_key);
				let proposals = lock.remove(&batch_key).unwrap(); // safe unwrap since lock is held
				std::mem::drop(lock);
				save_signed_proposals_in_storage::<B, C, BE>(
					&self.get_authority_public_key(),
					&self.current_validator_set,
					&self.latest_header,
					&self.backend,
					proposals,
				);
			} else {
				log::info!(target: "dkg", "{}/{} proposals have resolved for batch {:?}", proposals_for_this_batch.len(), batch_key.len, batch_key);
			}
		}

		Ok(())
	}

	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError> {
		gossip_public_key::<B, C, BE>(
			&self.keystore,
			&mut *self.gossip_engine.lock(),
			&mut *self.aggregated_public_keys.lock(),
			key,
		);
		Ok(())
	}

	fn store_public_key(
		&self,
		key: LocalKey<Secp256k1>,
		round_id: RoundId,
	) -> Result<(), DKGError> {
		let sr_pub = self.get_sr25519_public_key();
		if let Some(path) = self.local_key_path.clone() {
			store_localkey(key, round_id, Some(path), self.local_keystore.as_ref(), sr_pub)
				.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
		}

		Ok(())
	}

	fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError> {
		let now = self.latest_header.read().clone().ok_or_else(|| DKGError::CriticalError {
			reason: "latest header does not exist!".to_string(),
		})?;
		let at: BlockId<B> = BlockId::hash(now.hash());
		Ok(self
			.client
			.runtime_api()
			.get_signing_jailed(&at, (&*self.best_authorities).clone())
			.unwrap_or_default())
	}

	fn get_authority_set(&self) -> &Vec<Public> {
		&*self.best_authorities
	}
}

#[derive(Clone)]
pub struct TestDummyIface {
	pub sender: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<Public>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	// key is party_index, hash of data. Needed especially for local unit tests
	pub vote_results: Arc<Mutex<HashMap<BatchKey, Vec<(Proposal, SignatureRecid, BigInt)>>>>,
	pub keygen_key: Arc<Mutex<Option<LocalKey<Secp256k1>>>>,
}

impl BlockChainIface for TestDummyIface {
	type Clock = u32;
	fn verify_signature_against_authorities(
		&self,
		message: Arc<SignedDKGMessage<Public>>,
	) -> Result<DKGMessage<Public>, DKGError> {
		Ok(message.msg.clone())
	}

	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
		log::info!(
				"Sending message through iface id={}",
				unsigned_msg.payload.async_proto_only_get_sender_id().unwrap()
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
		unsigned_proposal: UnsignedProposal,
		round_id: RoundId,
		batch_key: BatchKey,
		message: BigInt,
	) -> Result<(), DKGError> {
		let mut lock = self.vote_results.lock();
		let _payload_key = unsigned_proposal.key;
		let signature = convert_signature(&signature_rec).ok_or_else(|| {
			DKGError::CriticalError { reason: "Unable to serialize signature".to_string() }
		})?;

		let finished_round = DKGSignedPayload {
			key: round_id.encode(),
			payload: "Webb".encode(),
			signature: signature.encode(),
		};

		let prop = make_signed_proposal(ProposalKind::EVM, finished_round).unwrap();
		lock.entry(batch_key).or_default().push((prop, signature_rec, message));

		Ok(())
	}

	fn gossip_public_key(&self, _key: DKGPublicKeyMessage) -> Result<(), DKGError> {
		// we do not gossip the public key in the test interface
		Ok(())
	}

	fn store_public_key(&self, key: LocalKey<Secp256k1>, _: RoundId) -> Result<(), DKGError> {
		*self.keygen_key.lock() = Some(key);
		Ok(())
	}

	fn get_jailed_signers_inner(&self) -> Result<Vec<Public>, DKGError> {
		Ok(vec![])
	}

	fn get_authority_set(&self) -> &Vec<Public> {
		&*self.best_authorities
	}
}
