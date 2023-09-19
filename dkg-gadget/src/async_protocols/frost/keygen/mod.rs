use crate::{
	async_protocols::{blockchain_interface::BlockchainInterface, types::LocalKeyType},
	dkg_modules::wt_frost::{validate_parameters, NetInterface},
};
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{gossip_messages::PublicKeyMessage, SessionId};
use std::{future::Future, pin::Pin};
use wsts::v2::Party;

/// `party_id`: Should be in the range [0, n). For the DKG, should be our index in the best
/// authorities starting from 0.
pub fn protocol<Net: NetInterface, BI: BlockchainInterface>(
	n: u32,
	party_id: u32,
	k: u32,
	t: u32,
	mut network: Net,
	bc: BI,
	session_id: SessionId,
) -> Pin<Box<dyn Future<Output = Result<(), DKGError>>>> {
	Box::pin(async move {
		validate_parameters(n, k, t)?;

		let mut rng = rand::thread_rng();
		let key_ids = crate::dkg_modules::wt_frost::generate_party_key_ids(n, k);
		let our_key_ids = key_ids
			.get(party_id as usize)
			.ok_or_else(|| DKGError::StartKeygen { reason: "Bad party_id".to_string() })?;

		let mut party = Party::new(party_id, our_key_ids, n, k, t, &mut rng);
		let public_key =
			crate::dkg_modules::wt_frost::run_dkg(&mut party, &mut rng, &mut network, n as usize)
				.await?;

		// Encode via serde_json
		let public_key_bytes = serde_json::to_vec(&public_key)
			.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;

		// Gossip the public key
		let pkey_message =
			PublicKeyMessage { session_id, pub_key: public_key_bytes, signature: vec![] };

		let state = party.save();

		// Store and gossip the public key
		bc.store_public_key(LocalKeyType::FROST(public_key, state), session_id)?;
		bc.gossip_public_key(pkey_message)?;

		Ok(())
	})
}
