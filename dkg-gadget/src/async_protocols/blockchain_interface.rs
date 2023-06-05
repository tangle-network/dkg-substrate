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
	debug_logger::DebugLogger,
	gossip_engine::GossipEngineIface,
	gossip_messages::{dkg_message::sign_and_send_messages, public_key_gossip::gossip_public_key},
	metrics::Metrics,
	storage::proposals::save_signed_proposals_in_storage,
	worker::{DKGWorker, HasLatestHeader, KeystoreExt, TestBundle},
	Client, DKGApi, DKGKeystore,
};
use codec::Encode;
use curv::{elliptic::curves::Secp256k1, BigInt};
use dkg_primitives::{
	types::{DKGError, DKGMessage, DKGPublicKeyMessage, SessionId, SignedDKGMessage},
	utils::convert_signature,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	AggregatedPublicKeys, AuthoritySet, BatchId, DKGSignedPayload, MaxAuthorities,
	MaxProposalLength, MaxProposalsInBatch, MaxSignatureLength, SignedProposalBatch,
	StoredUnsignedProposalBatch, UnsignedProposal,
};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::SignatureRecid, state_machine::keygen::LocalKey,
};
use parking_lot::RwLock;
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_runtime::traits::{Block, Get, NumberFor};
use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc};
use webb_proposals::Proposal;

use super::KeygenPartyId;

#[async_trait::async_trait]
#[auto_impl::auto_impl(Arc,&,&mut)]
pub trait BlockchainInterface: Send + Sync + Unpin {
	type Clock: Debug + AtLeast32BitUnsigned + Copy + Send + Sync;
	type GossipEngine: GossipEngineIface;
	type MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin;
	type BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin;
	type MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin;
	type MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin;

	async fn verify_signature_against_authorities(
		&self,
		message: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError>;
	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError>;
	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			Self::BatchId,
			Self::MaxProposalLength,
			Self::MaxProposalsInBatch,
			Self::Clock,
		>,
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
	fn get_authority_set(&self) -> Vec<(KeygenPartyId, Public)>;
	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine>;
	/// Returns the present time
	fn now(&self) -> Self::Clock;
}

pub struct DKGProtocolEngine<
	B: Block,
	BE,
	C,
	GE,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
	BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
> {
	pub backend: Arc<BE>,
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
	pub client: Arc<C>,
	pub keystore: DKGKeystore,
	pub db: Arc<dyn crate::db::DKGDbBackend>,
	pub gossip_engine: Arc<GE>,
	pub aggregated_public_keys: Arc<RwLock<HashMap<SessionId, AggregatedPublicKeys>>>,
	pub best_authorities: Arc<Vec<(KeygenPartyId, Public)>>,
	pub authority_public_key: Arc<Public>,
	pub vote_results: Arc<
		RwLock<
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
	>,
	pub is_genesis: bool,
	pub current_validator_set: Arc<RwLock<AuthoritySet<Public, MaxAuthorities>>>,
	pub local_keystore: Arc<RwLock<Option<Arc<LocalKeystore>>>>,
	pub metrics: Arc<Option<Metrics>>,
	pub test_bundle: Option<TestBundle>,
	pub logger: DebugLogger,
	pub _pd: PhantomData<(BE, BatchId, MaxProposalsInBatch, MaxSignatureLength)>,
}

impl<
		B: Block,
		BE,
		C,
		GE,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
		MaxAuthorities: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
		BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	>
	DKGProtocolEngine<
		B,
		BE,
		C,
		GE,
		MaxProposalLength,
		MaxAuthorities,
		BatchId,
		MaxProposalsInBatch,
		MaxSignatureLength,
	>
{
	fn send_result_to_test_client(&self, result: Result<(), String>, pub_key: Option<Vec<u8>>) {
		if let Some(bundle) = self.test_bundle.as_ref() {
			if let Some(current_test_id) = *bundle.current_test_id.read() {
				let _ = bundle
					.to_test_client
					.send((current_test_id, result, pub_key))
					.map_err(|err| format!("send_result_to_test_client failed with error: {err}"));
			}
		}
	}
}

impl<
		B: Block,
		BE,
		C,
		GE,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
		BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	> KeystoreExt
	for DKGProtocolEngine<
		B,
		BE,
		C,
		GE,
		MaxProposalLength,
		MaxAuthorities,
		BatchId,
		MaxProposalsInBatch,
		MaxSignatureLength,
	>
{
	fn get_keystore(&self) -> &DKGKeystore {
		&self.keystore
	}
}

impl<
		B,
		BE,
		C,
		GE,
		MaxProposalLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static,
		BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
		MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	> HasLatestHeader<B>
	for DKGProtocolEngine<
		B,
		BE,
		C,
		GE,
		MaxProposalLength,
		MaxAuthorities,
		BatchId,
		MaxProposalsInBatch,
		MaxSignatureLength,
	> where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface,
	C: Client<B, BE> + 'static,
{
	fn get_latest_header(&self) -> &Arc<RwLock<Option<B::Header>>> {
		&self.latest_header
	}
}

#[async_trait::async_trait]
impl<B, BE, C, GE> BlockchainInterface
	for DKGProtocolEngine<
		B,
		BE,
		C,
		GE,
		MaxProposalLength,
		MaxAuthorities,
		BatchId,
		MaxProposalsInBatch,
		MaxSignatureLength,
	> where
	B: Block,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
	BE: Backend<B> + Unpin + 'static,
	MaxProposalLength: Get<u32> + Send + Sync + Clone + 'static + std::fmt::Debug + Unpin,
	BatchId: Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	MaxProposalsInBatch: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	MaxSignatureLength: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static + Unpin,
	GE: GossipEngineIface + 'static,
{
	type Clock = NumberFor<B>;
	type GossipEngine = Arc<GE>;
	type MaxProposalLength = MaxProposalLength;
	type BatchId = BatchId;
	type MaxProposalsInBatch = MaxProposalsInBatch;
	type MaxSignatureLength = MaxSignatureLength;

	async fn verify_signature_against_authorities(
		&self,
		msg: SignedDKGMessage<Public>,
	) -> Result<DKGMessage<Public>, DKGError> {
		let client = &self.client;

		DKGWorker::<_, _, _, GE>::verify_signature_against_authorities_inner(
			&self.logger,
			msg,
			&self.latest_header,
			client,
		)
		.await
	}

	fn sign_and_send_msg(&self, unsigned_msg: DKGMessage<Public>) -> Result<(), DKGError> {
		sign_and_send_messages(self.gossip_engine.clone(), &self.keystore, unsigned_msg);
		Ok(())
	}

	fn process_vote_result(
		&self,
		signature: SignatureRecid,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			Self::BatchId,
			Self::MaxProposalLength,
			Self::MaxProposalsInBatch,
			Self::Clock,
		>,
		session_id: SessionId,
		batch_key: BatchKey,
		_message: BigInt,
	) -> Result<(), DKGError> {
		// Call worker.rs: handle_finished_round -> Proposal
		// aggregate Proposal into Vec<Proposal>
		self.logger.info(format!(
			"PROCESS VOTE RESULT : session_id {session_id:?}, signature : {signature:?}"
		));

		let signature = convert_signature(&signature).ok_or_else(|| DKGError::CriticalError {
			reason: "Unable to serialize signature".to_string(),
		})?;

		let signed_proposal_batch = SignedProposalBatch {
			batch_id: unsigned_proposal_batch.batch_id,
			proposals: unsigned_proposal_batch.proposals,
			signature: signature.encode().try_into().expect("Signature exceeds runtime bounds!"),
		};

		let mut lock = self.vote_results.write();
		let proposals_for_this_batch = lock.entry(batch_key).or_default();

		proposals_for_this_batch.push(signed_proposal_batch);

		if proposals_for_this_batch.len() == batch_key.len {
			self.logger.info(format!("All proposals have resolved for batch {batch_key:?}"));
			let proposals = lock.remove(&batch_key).expect("Cannot get lock on vote_results"); // safe unwrap since lock is held
			std::mem::drop(lock);

			if let Some(metrics) = self.metrics.as_ref() {
				metrics.dkg_signed_proposal_counter.inc_by(proposals.len() as u64);
			}

			save_signed_proposals_in_storage::<
				B,
				C,
				BE,
				MaxProposalLength,
				MaxAuthorities,
				BatchId,
				MaxProposalsInBatch,
				MaxSignatureLength,
			>(
				&self.get_authority_public_key(),
				&self.current_validator_set,
				&self.latest_header,
				&self.backend,
				proposals,
				&self.logger,
			);
			// send None to signify this was a signing result
			self.send_result_to_test_client(Ok(()), None);
		} else {
			self.logger.info(format!(
				"{}/{} proposals have resolved for batch {:?}",
				proposals_for_this_batch.len(),
				batch_key.len,
				batch_key,
			));
		}

		Ok(())
	}

	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError> {
		let public_key = key.pub_key.clone();
		gossip_public_key::<B, C, BE, GE>(
			&self.keystore,
			self.gossip_engine.clone(),
			&mut self.aggregated_public_keys.write(),
			key,
		);

		self.send_result_to_test_client(Ok(()), Some(public_key));

		Ok(())
	}

	fn store_public_key(
		&self,
		key: LocalKey<Secp256k1>,
		session_id: SessionId,
	) -> Result<(), DKGError> {
		self.logger.debug(format!("Storing local key for session {session_id:?}"));
		self.db.store_local_key(session_id, key)
	}

	fn get_authority_set(&self) -> Vec<(KeygenPartyId, Public)> {
		(*self.best_authorities).clone()
	}

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine> {
		Some(&self.gossip_engine)
	}

	fn now(&self) -> Self::Clock {
		self.get_latest_block_number()
	}
}
