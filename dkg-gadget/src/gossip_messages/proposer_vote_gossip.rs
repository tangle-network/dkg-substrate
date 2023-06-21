use crate::{gossip_engine::GossipEngineIface, worker::AggregatedProposerVotesStore};
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
//
// Handles non-dkg messages
use crate::{
	storage::proposer_votes::store_aggregated_proposer_votes,
	worker::{DKGWorker, KeystoreExt},
	Client,
};
use codec::Encode;
use dkg_primitives::types::{
	DKGError, DKGMessage, DKGMsgStatus, NetworkMsgPayload, SignedDKGMessage,
};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	ethereum_abi::IntoAbiToken,
	gossip_messages::{ProposerVoteMessage, PublicKeyMessage},
	AggregatedProposerVotes, DKGApi, MaxAuthorities, MaxProposalLength, MaxSignatureLength,
	MaxVoteLength, MaxVotes,
};
use sc_client_api::Backend;
use sp_runtime::{
	traits::{Block, Get, Header, NumberFor},
	BoundedVec,
};

pub(crate) fn handle_proposer_vote<B, BE, C, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	dkg_msg: DKGMessage<Public>,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	// Get authority accounts
	let header = &dkg_worker.latest_header.read().clone().ok_or(DKGError::NoHeader)?;
	let current_block_number = *header.number();
	let authorities = dkg_worker.validator_set(header).map(|a| (a.0.authorities, a.1.authorities));
	if authorities.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	if let NetworkMsgPayload::ProposerVote(msg) = dkg_msg.payload {
		dkg_worker
			.logger
			.debug(format!("SESSION {} | Received proposer vote broadcast", msg.session_id));

		let is_main_round = {
			if let Some(round) = dkg_worker.rounds.read().as_ref() {
				msg.session_id == round.session_id
			} else {
				false
			}
		};

		// Create encoded message
		let mut encoded_vote_msg =
			msg.encode_abi().try_into().map_err(|_| DKGError::InputOutOfBounds)?;

		let session_id = msg.session_id;
		let mut lock = dkg_worker.aggregated_proposer_votes.write();
		let votes = lock.entry((msg.session_id, msg.new_governor)).or_insert_with(|| {
			AggregatedProposerVotes {
				session_id: msg.session_id,
				encoded_vote: encoded_vote_msg,
				voters: Default::default(),
				signatures: Default::default(),
			}
		});
		// Fetch the current threshold for the DKG. We will use the
		// current threshold to determine if we have enough signatures
		// to submit the next DKG public key.
		let threshold = dkg_worker.get_next_signature_threshold(header) as usize;
		dkg_worker.logger.debug(format!(
			"SESSION {} | Threshold {} | Aggregated proposer votes {}",
			msg.session_id,
			threshold,
			votes.voters.len()
		));

		if votes.voters.len() > threshold {
			store_aggregated_proposer_votes(&dkg_worker, votes)?;
		} else {
			dkg_worker.logger.debug(format!(
				"SESSION {} | Need more signatures to submit next DKG public key, needs {} more",
				msg.session_id,
				(threshold + 1) - votes.voters.len()
			));
		}
	}

	Ok(())
}

pub(crate) fn gossip_proposer_vote<B, C, BE, GE>(
	dkg_worker: &DKGWorker<B, BE, C, GE>,
	msg: ProposerVoteMessage,
) -> Result<(), DKGError>
where
	B: Block,
	BE: Backend<B>,
	GE: GossipEngineIface,
	C: Client<B, BE>,
	MaxProposalLength: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	MaxAuthorities: Get<u32> + Clone + Send + Sync + 'static + std::fmt::Debug,
	C::Api: DKGApi<
		B,
		AuthorityId,
		<<B as Block>::Header as Header>::Number,
		MaxProposalLength,
		MaxAuthorities,
	>,
{
	let public = dkg_worker.key_store.get_authority_public_key();
	let encoded_vote: BoundedVec<u8, MaxVoteLength> =
		msg.encode_abi().try_into().map_err(|_| DKGError::InputOutOfBounds)?;

	if let Ok(signature) = dkg_worker.key_store.sign(&public, &encoded_vote) {
		let encoded_signature = signature.encode();
		let payload = NetworkMsgPayload::ProposerVote(ProposerVoteMessage {
			signature: encoded_signature.clone(),
			..msg.clone()
		});

		let status =
			if msg.session_id == 0u64 { DKGMsgStatus::ACTIVE } else { DKGMsgStatus::QUEUED };
		let message = DKGMessage::<AuthorityId> {
			sender_id: public.clone(),
			// we need to gossip the final public key to all parties, so no specific recipient in
			// this case.
			recipient_id: None,
			status,
			session_id: msg.session_id,
			payload,
		};
		let encoded_dkg_message = message.encode();

		crate::utils::inspect_outbound("proposer_vote", encoded_dkg_message.len());

		match dkg_worker.key_store.sign(&public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: message, signature: Some(sig.encode()) };
				if let Err(e) = dkg_worker.keygen_gossip_engine.gossip(signed_dkg_message) {
					dkg_worker
						.keygen_gossip_engine
						.logger()
						.error(format!("Failed to gossip DKG public key: {e:?}"));
				}
			},
			Err(e) => dkg_worker.logger.error(format!("🕸️  Error signing DKG message: {e:?}")),
		}

		let mut lock = dkg_worker.aggregated_proposer_votes.write();
		let bounded_voters: BoundedVec<Public, MaxAuthorities> =
			vec![public.clone()].try_into().map_err(|_| DKGError::InputOutOfBounds)?;
		let bounded_encoded_sig: BoundedVec<u8, MaxSignatureLength> =
			encoded_signature.clone().try_into().map_err(|_| DKGError::InputOutOfBounds)?;
		let bounded_sigs: BoundedVec<BoundedVec<u8, MaxSignatureLength>, MaxAuthorities> =
			vec![bounded_encoded_sig.clone()]
				.try_into()
				.map_err(|_| DKGError::InputOutOfBounds)?;
		let votes = lock.entry((msg.session_id, encoded_vote.to_vec())).or_insert_with(|| {
			AggregatedProposerVotes {
				session_id: msg.session_id,
				encoded_vote,
				voters: bounded_voters,
				signatures: bounded_sigs,
			}
		});

		if votes.voters.contains(&public) {
			return Ok(())
		}

		votes.voters.try_push(public).map_err(|_| DKGError::InputOutOfBounds)?;
		votes
			.signatures
			.try_push(encoded_signature.try_into().map_err(|_| DKGError::InputOutOfBounds)?)
			.map_err(|_| DKGError::InputOutOfBounds)?;

		dkg_worker
			.keygen_gossip_engine
			.logger()
			.debug(format!("Gossiping local node proposer vote for {:?} ", msg));
		Ok(())
	} else {
		dkg_worker
			.keygen_gossip_engine
			.logger()
			.error("Could not sign proposer vote".to_string());
		Err(DKGError::CannotSign)
	}
}
