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
	persistence::store_localkey,
	proposal::get_signed_proposal,
	storage::proposals::save_signed_proposals_in_storage,
	worker::{DKGWorker, HasLatestHeader, KeystoreExt},
	Client, DKGApi, DKGKeystore,
};
use codec::Encode;
use curv::{elliptic::curves::Secp256k1, BigInt};
use dkg_primitives::{
	types::{
		DKGError, DKGMessage, DKGPublicKeyMessage, DKGSignedPayload, RoundId, SignedDKGMessage,
	},
	utils::convert_signature,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	AggregatedPublicKeys, AuthoritySet, Proposal, UnsignedProposal,
};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::SignatureRecid, state_machine::keygen::LocalKey,
};
use parking_lot::{Mutex, RwLock};
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header, NumberFor},
};
use std::{collections::HashMap, marker::PhantomData, path::PathBuf, sync::Arc};

#[auto_impl::auto_impl(Arc,&,&mut)]
pub trait BlockchainInterface: Send + Sync {
	type Clock: AtLeast32BitUnsigned + Copy + Send + Sync;
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
		round_id: RoundId,
		batch_key: BatchKey,
		message: BigInt,
	) -> Result<(), DKGError>;
	fn gossip_public_key(&self, key: DKGPublicKeyMessage) -> Result<(), DKGError>;
	fn store_public_key(&self, key: LocalKey<Secp256k1>, round_id: RoundId)
		-> Result<(), DKGError>;
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

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine>;
	/// Returns the present time
	fn now(&self) -> Self::Clock;
}

pub struct DKGProtocolEngine<B: Block, BE, C, GE> {
	pub backend: Arc<BE>,
	pub latest_header: Arc<RwLock<Option<B::Header>>>,
	pub client: Arc<C>,
	pub keystore: DKGKeystore,
	pub gossip_engine: Arc<GE>,
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
			(&*msg).clone(),
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
		round_id: RoundId,
		batch_key: BatchKey,
		_message: BigInt,
	) -> Result<(), DKGError> {
		// Call worker.rs: handle_finished_round -> Proposal
		// aggregate Proposal into Vec<Proposal>
		let payload_key = unsigned_proposal.key;
		let signature = convert_signature(&signature).ok_or_else(|| DKGError::CriticalError {
			reason: "Unable to serialize signature".to_string(),
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
		gossip_public_key::<B, C, BE, GE>(
			&self.keystore,
			self.gossip_engine.clone(),
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

	fn get_gossip_engine(&self) -> Option<&Self::GossipEngine> {
		Some(&self.gossip_engine)
	}

	fn now(&self) -> Self::Clock {
		self.get_latest_block_number()
	}
}
