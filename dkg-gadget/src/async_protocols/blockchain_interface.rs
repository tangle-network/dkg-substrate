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

use crate::{
	async_protocols::BatchKey,
	gossip_engine::GossipEngineIface,
	gossip_messages::{dkg_message::sign_and_send_messages, public_key_gossip::gossip_public_key},
	metrics::Metrics,
	proposal::get_signed_proposal,
	storage::proposals::save_signed_proposals_in_storage,
	worker::{DKGWorker, HasLatestHeader, KeystoreExt},
	Client, DKGApi, DKGKeystore,
};
use codec::Encode;
use curv::{elliptic::curves::Secp256k1, BigInt};
use dkg_primitives::{
	types::{
		DKGError, DKGMessage, DKGPublicKeyMessage, DKGSignedPayload, SessionId, SignedDKGMessage,
	},
	utils::convert_signature,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	AggregatedPublicKeys, AuthoritySet, UnsignedProposal,
};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::SignatureRecid, state_machine::keygen::LocalKey,
};
use parking_lot::RwLock;
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_runtime::traits::{Block, NumberFor};
use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc};
use webb_proposals::Proposal;

#[auto_impl::auto_impl(Arc,&,&mut)]
pub trait BlockchainInterface: Send + Sync {
	type Clock: Debug + AtLeast32BitUnsigned + Copy + Send + Sync;
	type GossipEngine: GossipEngineIface;

	fn verify_signature_against_authorities(
		&self,
		message: Arc<SignedDKGMessage<Public>>,
	) -> Result<DKGMessage<Public>, DKGError>;
	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError>;
	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal: UnsignedProposal,
		session_id: SessionId,
		batch_key: BatchKey,
		message: BigInt,
	) -> Result<(), DKGError>;
	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError>;
	fn store_public_key(
		&self,
		key: LocalKey<Secp256k1>,
		session_id: SessionId,
	) -> Result<(), DKGError>;
	fn get_authority_set(&self) -> &Vec<Public>;
	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine>;
	/// Returns the present time
	fn now(&self) -> Self::Clock;
}

pub struct DKGProtocolEngine<B: Block, BE, C, GE> {
	pub backend: Arc<BE>,
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
	pub client: Arc<C>,
	pub keystore: DKGKeystore,
	pub db: Arc<dyn crate::db::DKGDbBackend>,
	pub gossip_engine: Arc<GE>,
	pub aggregated_public_keys: Arc<RwLock<HashMap<SessionId, AggregatedPublicKeys>>>,
	pub best_authorities: Arc<Vec<Public>>,
	pub authority_public_key: Arc<Public>,
	pub vote_results: Arc<RwLock<HashMap<BatchKey, Vec<Proposal>>>>,
	pub is_genesis: bool,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public>>>,
	pub local_keystore: Arc<RwLock<Option<Arc<LocalKeystore>>>>,
	pub metrics: Arc<Option<Metrics>>,
	pub _pd: PhantomData<BE>,
}

impl<B: Block, BE, C, GE> KeystoreExt for DKGProtocolEngine<B, BE, C, GE> {
	fn get_keystore(&self) -> &DKGKeystore {
		&self.keystore
	}
}

impl<B, BE, C, GE> HasLatestHeader<B> for DKGProtocolEngine<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
{
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}
}

impl<B, BE, C, GE> BlockchainInterface for DKGProtocolEngine<B, BE, C, GE>
where
	B: Block,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>>,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
{
	type Clock = NumberFor<B>;
	type GossipEngine = Arc<GE>;

	fn verify_signature_against_authorities(
		&self,
		msg: Arc<SignedDKGMessage<Public>>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let client = &self.client;

		DKGWorker::<_, _, _, GE>::verify_signature_against_authorities_inner(
			(*msg).clone(),
			&self.latest_header,
			client,
		)
	}

	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
		sign_and_send_messages(self.gossip_engine.clone(), &self.keystore, unsigned_msg);
		Ok(())
	}

	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal: UnsignedProposal,
		session_id: SessionId,
		batch_key: BatchKey,
		_message: BigInt,
	) -> Result<(), DKGError> {
		// Call worker.rs: handle_finished_round -> Proposal
		// aggregate Proposal into Vec<Proposal>
		dkg_logging::info!(target: "dkg", "PROCESS VOTE RESULT : session_id {:?}, signature : {:?}", session_id, signature);
		let payload_key = unsigned_proposal.key;
		let signature = convert_signature(&signature).ok_or_else(|| DKGError::CriticalError {
			reason: "Unable to serialize signature".to_string(),
		})?;

		let finished_round = DKGSignedPayload {
			key: session_id.encode(),
			payload: unsigned_proposal.data().clone(),
			signature: signature.encode(),
		};

		let mut lock = self.vote_results.write();
		let proposals_for_this_batch = lock.entry(batch_key).or_default();

		if let Some(proposal) =
			get_signed_proposal::<B, C, BE>(&self.backend, finished_round, payload_key)
		{
			proposals_for_this_batch.push(proposal);

			if proposals_for_this_batch.len() == batch_key.len {
				dkg_logging::info!(target: "dkg", "All proposals have resolved for batch {:?}", batch_key);
				let proposals = lock.remove(&batch_key).unwrap(); // safe unwrap since lock is held
				std::mem::drop(lock);

				if let Some(metrics) = self.metrics.as_ref() {
					metrics.dkg_signed_proposal_counter.inc_by(proposals.len() as u64);
				}

				save_signed_proposals_in_storage::<B, C, BE>(
					&self.get_authority_public_key(),
					&self.current_validator_set,
					&self.latest_header,
					&self.backend,
					proposals,
				);
			} else {
				dkg_logging::info!(target: "dkg", "{}/{} proposals have resolved for batch {:?}", proposals_for_this_batch.len(), batch_key.len, batch_key);
			}
		}

		Ok(())
	}

	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError> {
		gossip_public_key::<B, C, BE, GE>(
			&self.keystore,
			self.gossip_engine.clone(),
			&mut self.aggregated_public_keys.write(),
			key,
		);
		Ok(())
	}

	fn store_public_key(
		&self,
		key: LocalKey<Secp256k1>,
		session_id: SessionId,
	) -> Result<(), DKGError> {
		self.db.store_local_key(session_id, key)
	}

	fn get_authority_set(&self) -> &Vec<Public> {
		&self.best_authorities
	}

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine> {
		Some(&self.gossip_engine)
	}

	fn now(&self) -> Self::Clock {
		self.get_latest_block_number()
	}
}
