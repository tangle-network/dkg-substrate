// Handles non-dkg messages

use sc_client_api::Backend;
use sp_runtime::generic::BlockId;
use sp_runtime::traits::{Block, Header};
use dkg_primitives::crypto::Public;
use crate::worker::DKGWorker;
use dkg_primitives::types::{DKGError, DKGMessage, DKGMsgPayload};
use dkg_runtime_primitives::AggregatedPublicKeys;
use dkg_runtime_primitives::crypto::AuthorityId;
use dkg_runtime_primitives::DKGApi;
use crate::Client;
use log::debug;

pub(crate) fn handle_public_key_broadcast<B, C, BE>(mut dkg_worker: &mut DKGWorker<B, C, BE>, dkg_msg: DKGMessage<Public>) -> Result<(), DKGError>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		C::Api: DKGApi<B, AuthorityId, <<B as Block>::Header as Header>::Number> {
	if !dkg_worker.dkg_state.listening_for_pub_key && !dkg_worker.dkg_state.listening_for_active_pub_key {
		return Err(DKGError::GenericError {
			reason: "Not listening for public key broadcast".to_string(),
		})
	}

	// Get authority accounts
	let header = dkg_worker.latest_header.as_ref().ok_or(DKGError::NoHeader)?;
	let current_block_number = header.number().clone();
	let at: BlockId<B> = BlockId::hash(header.hash());
	let authority_accounts = dkg_worker.client.runtime_api().get_authority_accounts(&at).ok();
	if authority_accounts.is_none() {
		return Err(DKGError::NoAuthorityAccounts)
	}

	match dkg_msg.payload {
		DKGMsgPayload::PublicKeyBroadcast(msg) => {
			debug!(target: "dkg", "Received public key broadcast");

			let is_main_round = {
				if dkg_worker.rounds.is_some() {
					msg.round_id == dkg_worker.rounds.as_ref().unwrap().get_id()
				} else {
					false
				}
			};

			dkg_worker.authenticate_msg_origin(
				is_main_round,
				authority_accounts.unwrap(),
				&msg.pub_key,
				&msg.signature,
			)?;

			let key_and_sig = (msg.pub_key, msg.signature);
			let round_id = msg.round_id;
			let mut aggregated_public_keys = match dkg_worker.aggregated_public_keys.get(&round_id) {
				Some(keys) => keys.clone(),
				None => AggregatedPublicKeys::default(),
			};

			if !aggregated_public_keys.keys_and_signatures.contains(&key_and_sig) {
				aggregated_public_keys.keys_and_signatures.push(key_and_sig);
				dkg_worker.aggregated_public_keys.insert(round_id, aggregated_public_keys.clone());
			}
			// Fetch the current threshold for the DKG. We will use the
			// current threshold to determine if we have enough signatures
			// to submit the next DKG public key.
			let threshold = dkg_worker.get_threshold(header).unwrap() as usize;
			if aggregated_public_keys.keys_and_signatures.len() >= threshold {
				dkg_worker.store_aggregated_public_keys(
					is_main_round,
					round_id,
					&aggregated_public_keys,
					current_block_number,
				)?;
			}
		},
		_ => {},
	}

	Ok(())
}
