use crate::{
	messages::public_key_gossip::gossip_public_key, persistence::store_localkey, types::dkg_topic,
	worker::DKGWorker, Client,
};
use codec::Encode;
use dkg_primitives::{
	crypto::Public,
	rounds::MultiPartyECDSARounds,
	types::{
		DKGError, DKGMessage, DKGMisbehaviourMessage, DKGMsgPayload, DKGPublicKeyMessage,
		DKGResult, RoundId, SignedDKGMessage,
	},
};
use dkg_runtime_primitives::{
	crypto::AuthorityId, AggregatedMisbehaviourReports, AggregatedPublicKeys, DKGApi,
};
use log::{debug, error, trace};
use sc_client_api::Backend;
use sp_runtime::{
	generic::BlockId,
	traits::{Block, Header, NumberFor},
};

/// Sends outgoing dkg messages
pub(crate) fn send_outgoing_dkg_messages<B, C, BE>(mut dkg_worker: &mut DKGWorker<B, C, BE>)
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	debug!(target: "dkg", "🕸️  Try sending DKG messages");

	let mut keys_to_gossip = Vec::new();
	let mut rounds_send_result = vec![];
	let mut next_rounds_send_result = vec![];

	if let Some(mut rounds) = dkg_worker.rounds.take() {
		if let Some(id) = dkg_worker
			.key_store
			.authority_id(dkg_worker.current_validator_set.authorities.as_slice())
		{
			debug!(target: "dkg", "🕸️  Local authority id: {:?}", id.clone());
			rounds_send_result =
				send_messages(dkg_worker, &mut rounds, id, dkg_worker.get_latest_block_number());
		} else {
			error!("No local accounts available. Consider adding one via `author_insertKey` RPC.");
		}

		if dkg_worker.active_keygen_in_progress {
			let is_keygen_finished = rounds.is_keygen_finished();
			if is_keygen_finished {
				debug!(target: "dkg", "🕸️  Genesis DKGs keygen has completed");
				dkg_worker.active_keygen_in_progress = false;
				let pub_key = rounds.get_public_key().unwrap().to_bytes(true).to_vec();
				let round_id = rounds.get_id();
				keys_to_gossip.push((round_id, pub_key));
			}
		}

		dkg_worker.rounds = Some(rounds);
	}

	// Check if a there's a key gen process running for the queued authority set
	if dkg_worker.queued_keygen_in_progress {
		if let Some(id) = dkg_worker
			.key_store
			.authority_id(dkg_worker.queued_validator_set.authorities.as_slice())
		{
			debug!(target: "dkg", "🕸️  Local authority id: {:?}", id.clone());
			if let Some(mut next_rounds) = dkg_worker.next_rounds.take() {
				next_rounds_send_result = send_messages(
					dkg_worker,
					&mut next_rounds,
					id,
					dkg_worker.get_latest_block_number(),
				);

				let is_keygen_finished = next_rounds.is_keygen_finished();
				if is_keygen_finished {
					debug!(target: "dkg", "🕸️  Queued DKGs keygen has completed");
					dkg_worker.queued_keygen_in_progress = false;
					let pub_key = next_rounds.get_public_key().unwrap().to_bytes(true).to_vec();
					keys_to_gossip.push((next_rounds.get_id(), pub_key));
				}
				dkg_worker.next_rounds = Some(next_rounds);
			}
		} else {
			error!("No local accounts available. Consider adding one via `author_insertKey` RPC.");
		}
	}

	for (round_id, pub_key) in &keys_to_gossip {
		gossip_public_key(&mut dkg_worker, pub_key.clone(), *round_id);
	}

	for res in &rounds_send_result {
		if let Err(err) = res {
			dkg_worker.handle_dkg_error(err.clone());
		}
	}

	for res in &next_rounds_send_result {
		if let Err(err) = res {
			dkg_worker.handle_dkg_error(err.clone());
		}
	}
}

/// send actual messages
fn send_messages<B, C, BE>(
	mut dkg_worker: &mut DKGWorker<B, C, BE>,
	rounds: &mut MultiPartyECDSARounds<NumberFor<B>>,
	authority_id: Public,
	at: NumberFor<B>,
) -> Vec<Result<DKGResult, DKGError>>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number>,
{
	let results = rounds.proceed(at);

	for result in &results {
		if let Ok(DKGResult::KeygenFinished { round_id, local_key }) = result.clone() {
			if let Some(local_keystore) = dkg_worker.local_keystore.clone() {
				if let Some(local_key_path) = rounds.get_local_key_path() {
					let _ = store_localkey(
						local_key,
						round_id,
						local_key_path,
						dkg_worker.key_store.clone(),
						local_keystore,
					);
				}
			}
		}
	}

	for message in rounds.get_outgoing_messages() {
		let dkg_message =
			DKGMessage { id: authority_id.clone(), payload: message, round_id: rounds.get_id() };
		let encoded_dkg_message = dkg_message.encode();

		let maybe_sr25519_public = dkg_worker
			.key_store
			.sr25519_authority_id(&dkg_worker.key_store.sr25519_public_keys().unwrap_or_default());
		let sr25519_public = match maybe_sr25519_public {
			Some(sr25519_public) => sr25519_public,
			None => {
				error!(target: "dkg", "🕸️  Could not find sr25519 key in keystore");
				break
			},
		};

		match dkg_worker.key_store.sr25519_sign(&sr25519_public, &encoded_dkg_message) {
			Ok(sig) => {
				let signed_dkg_message =
					SignedDKGMessage { msg: dkg_message, signature: Some(sig.encode()) };
				let encoded_signed_dkg_message = signed_dkg_message.encode();
				dkg_worker.gossip_engine.lock().gossip_message(
					dkg_topic::<B>(),
					encoded_signed_dkg_message.clone(),
					true,
				);
			},
			Err(e) => trace!(
				target: "dkg",
				"🕸️  Error signing DKG message: {:?}",
				e
			),
		}
		trace!(target: "dkg", "🕸️  Sent DKG Message of len {}", encoded_dkg_message.len());
	}
	results
}
