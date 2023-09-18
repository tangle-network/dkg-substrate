use dkg_primitives::types::{DKGError, SSID};
use itertools::Itertools;
use rand::{CryptoRng, RngCore};
use std::{collections::HashMap, fmt::Debug};
use std::sync::Arc;
use async_trait::async_trait;
use sc_client_api::Backend;
use serde::{Deserialize, Serialize};
use sp_arithmetic::traits::SaturatedConversion;
use sp_runtime::traits::Block;
use wsts::{
	common::{PolyCommitment, PublicNonce, Signature, SignatureShare},
	v2,
	v2::SignatureAggregator,
	Scalar,
};
use crate::async_protocols::blockchain_interface::DKGProtocolEngine;
use crate::async_protocols::frost::keygen::FrostKeygenNetworkWrapper;
use crate::async_protocols::remote::AsyncProtocolRemote;
use crate::Client;
use crate::dkg_modules::{DKG, KeygenProtocolSetupParameters, ProtocolInitReturn, SigningProtocolSetupParameters};
use crate::gossip_engine::GossipEngineIface;
use crate::worker::{DKGWorker, HasLatestHeader, ProtoStageType};

/// DKG module for Weighted Threshold Frost
pub struct WTFrostDKG<B, BE, C, GE>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		GE: GossipEngineIface,
{
	pub(super) dkg_worker: DKGWorker<B, BE, C, GE>,
}

#[async_trait]
impl<B, BE, C, GE> DKG<B> for WTFrostDKG<B, BE, C, GE>
	where
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		GE: GossipEngineIface,
{
	async fn initialize_keygen_protocol(
		&self,
		params: KeygenProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		if let KeygenProtocolSetupParameters::WTFrost {
			best_authorities,
			authority_id,
			authority_public_key,
			keygen_protocol_hash,
			threshold,
			session_id,
			associated_block,
			stage
		} = params {
			// We must construct a remote and a task to ensure compatibility with the work manager
			// and future message delivery to the service
			let n = best_authorities.len() as u32;
			// For now, assume each party member owns n keys
			let k = n;
			// The party id for frost must be in [0, n). Thus, we just find out index in the best authorities
			const KEYGEN_SSID: SSID = 0;
			let party_id = best_authorities
				.iter()
				.find_position(|r| &r.1 == &authority_public_key)
				.map(|r| r.0 as u32)
				.expect("Authority ID not found in best authorities");
			let at = self.dkg_worker.get_latest_block_number();
			let associated_block_id = associated_block.saturated_into();
			let authority_mapping = Default::default(); // TODO

			// Setup the remote to allow communication between the async protocol and the DKG worker
			let remote = AsyncProtocolRemote::new(at, session_id, self.dkg_worker.logger.clone(), associated_block_id, KEYGEN_SSID, stage, authority_mapping);

			// Setup the engine to allow communication between the async protocol and the blockchain
			let bc_iface = Arc::new(DKGProtocolEngine {
				backend: self.dkg_worker.backend.clone(),
				latest_header: self.dkg_worker.latest_header.clone(),
				client: self.dkg_worker.client.clone(),
				keystore: self.dkg_worker.key_store.clone(),
				db: self.dkg_worker.db.clone(),
				gossip_engine: self.dkg_worker.gossip_engine.clone(),
				aggregated_public_keys: self.dkg_worker.aggregated_public_keys.clone(),
				current_validator_set: self.dkg_worker.current_validator_set.clone(),
				local_keystore: self.dkg_worker.local_keystore.clone(),
				vote_results: Arc::new(Default::default()),
				is_genesis: stage == ProtoStageType::KeygenGenesis,
				metrics: self.dkg_worker.metrics.clone(),
				test_bundle: self.dkg_worker.test_bundle.clone(),
				logger: self.dkg_worker.logger.clone(),
				_pd: Default::default(),
			});

			let network = FrostKeygenNetworkWrapper::new(self.dkg_worker.clone(), bc_iface.clone(), remote.clone(), authority_id, keygen_protocol_hash);

			let task = crate::async_protocols::frost::keygen::proto::protocol(n, party_id, k, threshold, network, bc_iface, session_id);

			Some((remote, task))
		} else {
			None
		}
	}

	async fn initialize_signing_protocol(
		&self,
		_params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError> {
		todo!()
	}

	fn can_handle_keygen_request(&self, params: &KeygenProtocolSetupParameters<B>) -> bool {
		matches!(params, KeygenProtocolSetupParameters::WTFrost { .. })
	}

	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool {
		matches!(params, SigningProtocolSetupParameters::WTFrost { .. })
	}
}

pub async fn run_dkg<RNG: RngCore + CryptoRng, Net: NetInterface>(
	signer: &mut v2::Party,
	rng: &mut RNG,
	net: &mut Net,
	n_signers: usize,
) -> Result<Vec<PolyCommitment>, DKGError> {
	// Broadcast our party_id, shares, and key_ids to each other
	let party_id = signer.party_id;
	let shares: HashMap<u32, Scalar> = signer.get_shares().into_iter().collect();
	let key_ids = signer.key_ids.clone();
	let poly_commitment = signer.get_poly_commitment(rng);
	let message = FrostMessage::DKG {
		party_id,
		shares: shares.clone(),
		key_ids: key_ids.clone(),
		poly_commitment: poly_commitment.clone(),
	};

	// Send the message
	net.send_message(message).await.map_err(|err| DKGError::GenericError {
		reason: format!("Error sending FROST message: {err:?}"),
	})?;

	let mut received_shares = HashMap::new();
	let mut received_key_ids = HashMap::new();
	let mut received_poly_commitments = HashMap::new();
	// insert our own shared into the received map
	received_shares.insert(party_id, shares);
	received_key_ids.insert(party_id, key_ids);
	received_poly_commitments.insert(party_id, poly_commitment);

	// Wait for n_signers to send their messages to us
	while received_shares.len() < n_signers {
		match net.next_message().await {
			Ok(Some(FrostMessage::DKG { party_id, shares, key_ids, poly_commitment })) => {
				received_shares.insert(party_id, shares);
				received_key_ids.insert(party_id, key_ids);
				received_poly_commitments.insert(party_id, poly_commitment);
			},

			Ok(Some(_)) | Err(_) => {},
			None =>
				return Err(DKGError::GenericError {
					reason: "NetListen connection died".to_string(),
				}),
		}
	}

	// Generate the party_shares: for each key id we own, we take our received key share at that
	// index
	let party_shares = signer
		.key_ids
		.iter()
		.copied()
		.map(|key_id| {
			let mut key_shares = HashMap::new();

			for (id, shares) in &received_shares {
				key_shares.insert(*id, shares[&key_id]);
			}

			(key_id, key_shares.into_iter().collect())
		})
		.collect();
	let polys = received_poly_commitments
		.iter()
		.sorted_by(|a, b| a.0.cmp(&b.0))
		.map(|r| r.1.clone())
		.collect_vec();
	signer
		.compute_secret(&party_shares, &polys)
		.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;
	Ok(polys)
}

/// `threshold`: Should be the number of participants in this round, since we stop looking for messages
/// after finding the first `t` messages
pub async fn run_signing<RNG: RngCore + CryptoRng, Net: NetInterface>(
	signer: &mut v2::Party,
	rng: &mut RNG,
	msg: &[u8],
	net: &mut Net,
	num_keys: u32,
	threshold: u32,
	public_key: Vec<PolyCommitment>,
) -> Result<Signature, DKGError> {
	// Broadcast the party_id, key_ids, and nonce to each other
	let nonce = signer.gen_nonce(rng);
	let party_id = signer.party_id;
	let key_ids = signer.key_ids.clone();
	let message = FrostMessage::Sign { party_id, key_ids: key_ids.clone(), nonce: nonce.clone() };

	// Send the message
	net.send_message(message).await.map_err(|err| DKGError::GenericError {
		reason: format!("Error sending FROST message: {err:?}"),
	})?;

	let mut party_key_ids = HashMap::new();
	let mut party_nonces = HashMap::new();

	party_key_ids.insert(party_id, key_ids);
	party_nonces.insert(party_id, nonce);

	while party_nonces.len() < threshold as usize {
		match net.next_message().await {
			Ok(Some(FrostMessage::Sign { party_id: party_id_recv, key_ids, nonce })) => {
				party_key_ids.insert(party_id_recv, key_ids);
				party_nonces.insert(party_id_recv, nonce);
			},

			Ok(Some(_)) | Err(_) => {},
			None =>
				return Err(DKGError::GenericError {
					reason: "NetListen connection died".to_string(),
				}),
		}
	}

	// Sort the vecs
	let party_ids = (0..threshold).into_iter().collect_vec();
	let party_key_ids = party_key_ids
		.into_iter()
		.sorted_by(|a, b| a.0.cmp(&b.0))
		.flat_map(|r| r.1)
		.collect_vec();
	let party_nonces = party_nonces
		.into_iter()
		.sorted_by(|a, b| a.0.cmp(&b.0))
		.map(|r| r.1)
		.collect_vec();

	// Generate our signature share
	let signature_share = signer.sign(msg, &party_ids, &party_key_ids, &party_nonces);
	let message = FrostMessage::SignFinal { party_id, signature_share: signature_share.clone() };
	// Broadcast our signature share to each other
	net.send_message(message).await.map_err(|err| DKGError::GenericError {
		reason: format!("Error sending FROST message: {err:?}"),
	})?;

	let mut signature_shares = HashMap::new();
	signature_shares.insert(party_id, signature_share.clone());

	// Receive n_signers number of shares
	while signature_shares.len() < threshold as usize {
		match net.next_message().await {
			Ok(Some(FrostMessage::SignFinal { party_id, signature_share })) => {
				signature_shares.insert(party_id, signature_share);
			},

			Ok(Some(_)) | Err(_) => {},
			None =>
				return Err(DKGError::GenericError {
					reason: "NetListen connection died".to_string(),
				}),
		}
	}

	// Sort the signature shares
	let signature_shares = signature_shares
		.into_iter()
		.sorted_by(|a, b| a.0.cmp(&b.0))
		.map(|r| r.1)
		.collect_vec();

	// Aggregate and sign to generate the signature
	let mut sig_agg = SignatureAggregator::new(num_keys, threshold, public_key)
		.map_err(|err| DKGError::GenericError { reason: err.to_string() })?;

	sig_agg
		.sign(msg, &party_nonces, &signature_shares, &party_key_ids)
		.map_err(|err| DKGError::GenericError { reason: err.to_string() })
}

pub fn create_signer_key_ids(signer_id: u32, keys_per_signer: u32) -> Vec<u32> {
	(0..keys_per_signer).map(|i| keys_per_signer * signer_id + i).collect()
}

/// Returns a Vec of indices that denotes which indexes within the public key vector
/// are owned by which party.
///
/// E.g., if n=4 and k=10,
///
/// let party_key_ids: Vec<Vec<u32>> = [
///     [0, 1, 2].to_vec(),
///     [3, 4].to_vec(),
///     [5, 6, 7].to_vec(),
///     [8, 9].to_vec(),
/// ]
///
/// In the above case, we go up from 0..=9 possible key ids since k=10, and
/// we have 4 grouping since n=4. We need to generalize this below
#[allow(dead_code)]
pub fn generate_party_key_ids(n: u32, k: u32) -> Vec<Vec<u32>> {
	let mut result = Vec::with_capacity(n as usize);
	let ids_per_party = k / n;
	let mut start = 0;

	for _ in 0..n {
		let end = start + ids_per_party;
		let ids = (start..end).collect();
		result.push(ids);
		start = end;
	}

	result
}

pub fn validate_parameters(n: u32, k: u32, t: u32) -> Result<(), DKGError> {
	if k & n != 0 {
		return Err(DKGError::GenericError { reason: "K % N != 0".to_string() })
	}

	if k == 0 {
		return Err(DKGError::GenericError { reason: "K == 0".to_string() })
	}

	if n <= t {
		return Err(DKGError::GenericError { reason: "N <= T".to_string() })
	}

	Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FrostMessage {
	DKG {
		party_id: u32,
		shares: HashMap<u32, Scalar>,
		key_ids: Vec<u32>,
		poly_commitment: PolyCommitment,
	},
	Sign {
		party_id: u32,
		key_ids: Vec<u32>,
		nonce: PublicNonce,
	},
	SignFinal {
		party_id: u32,
		signature_share: SignatureShare,
	},
}

#[async_trait::async_trait]
pub trait NetInterface {
	type Error: Debug;

	async fn next_message(&mut self) -> Result<Option<FrostMessage>, Self::Error>;
	async fn send_message(&mut self, msg: FrostMessage) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
	use crate::dkg_modules::wt_frost::{FrostMessage, NetInterface};
	use futures::{stream::FuturesUnordered, TryStreamExt};

	struct TestNetworkLayer {
		tx: tokio::sync::broadcast::Sender<FrostMessage>,
		rx: tokio::sync::broadcast::Receiver<FrostMessage>,
	}

	#[async_trait::async_trait]
	impl NetInterface for TestNetworkLayer {
		type Error = tokio::sync::broadcast::error::SendError<FrostMessage>;

		async fn next_message(&mut self) -> Result<Option<FrostMessage>, Self::Error> {
			Ok(self.rx.recv().await.ok())
		}

		async fn send_message(&mut self, msg: FrostMessage) -> Result<(), Self::Error> {
			self.tx.send(msg).map(|_| ())
		}
	}

	#[tokio::test]
	async fn test_n3t2k3() {
		test_inner::<3, 2, 3>().await;
	}

	async fn test_inner<const N: u32, const T: u32, const K: u32>() {
		dkg_logging::setup_log();
		assert_eq!(K % N, 0); // Enforce that each party owns the same number of keys
		assert_ne!(K, 0); // Enforce that K is not zero
		assert!(N > T);

		// Each node creates their own party
		let mut parties = Vec::new();
		let indices = super::generate_party_key_ids(N, K);
		let rng = &mut rand::thread_rng();
		// In reality, the idx below would be our index in the best authorities, starting from zero
		for (idx, key_indexes_owned_by_this_party) in indices.into_iter().enumerate() {
			// See https://github.com/Trust-Machines/wsts/blob/037e2eb4105cf9f9b1c034ee5c1540a40123b530/src/v2.rs#L515
			// for generating the party key IDS
			//let key_indexes_owned_by_this_party = super::create_signer_key_ids(idx, K);
			dkg_logging::info!(target: "dkg", "keys owned by party {idx}: {key_indexes_owned_by_this_party:?}");
			parties.push(wsts::v2::Party::new(
				idx as _,
				&key_indexes_owned_by_this_party,
				N,
				K,
				T,
				rng,
			));
		}

		// setup the network
		let (tx, _) = tokio::sync::broadcast::channel(1000);
		let mut networks = (0..N)
			.into_iter()
			.map(|_idx| TestNetworkLayer {
				tx: tx.clone(),
				rx: tx.subscribe(),
			})
			.collect::<Vec<_>>();

		// Test the DKG
		let dkgs = FuturesUnordered::new();
		for (party, network) in parties.iter_mut().zip(networks.iter_mut()) {
			dkgs.push(Box::pin(async move {
				let mut rng = rand::thread_rng();
				crate::dkg_modules::wt_frost::run_dkg(party, &mut rng, network, N as _).await
			}));
		}

		let mut public_keys = dkgs.try_collect::<Vec<_>>().await.unwrap();
		for public_key in &public_keys {
			assert_eq!(public_key.len(), N as usize);
			for public_key0 in &public_keys {
				// Assert all equal
				assert!(public_key
					.iter()
					.zip(public_key0)
					.all(|r| r.0.id.kG == r.1.id.kG &&
						r.0.id.id == r.1.id.id && r.0.id.kca == r.1.id.kca &&
						r.0.A == r.1.A));
			}
		}

		let public_key = public_keys.pop().unwrap();

		// Test the signing over an arbitrary message
		let msg = b"Hello, world!";

		// Start by choosing signers. Since our indexes, in reality, will be based on the set of
		// best authorities, we will choose the best of the best of authorities, so from 0..T
		let signers = FuturesUnordered::new();

		for (party, network) in parties.iter_mut().zip(networks.iter_mut()).take(T as _) {
			let public_key = public_key.clone();
			signers.push(Box::pin(async move {
				let mut rng = rand::thread_rng();
				crate::dkg_modules::wt_frost::run_signing(
					party, &mut rng, &*msg, network, K, T, public_key,
				)
				.await
			}));
		}

		let signatures = signers.try_collect::<Vec<_>>().await.unwrap();
		for signature0 in &signatures {
			for signature1 in &signatures {
				assert_eq!(signature0.R, signature1.R);
				assert_eq!(signature0.z, signature1.z);
			}
		}
	}
}
