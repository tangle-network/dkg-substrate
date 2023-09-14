use crate::{
	dkg_modules::{
		KeygenProtocolSetupParameters, ProtocolInitReturn, SigningProtocolSetupParameters, DKG,
	},
	gossip_engine::GossipEngineIface,
	worker::DKGWorker,
	Client,
};
use async_trait::async_trait;
use dkg_primitives::types::DKGError;
use sc_client_api::Backend;
use sp_runtime::traits::Block;

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
		_params: KeygenProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		todo!()
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

#[cfg(test)]
mod tests {
	use frost_coordinator::coordinator::{Command, Coordinator};
	use frost_signer::{
		config::{Config, PublicKeys},
		net::{Message, NetListen},
		signer::Signer,
		signing_round::wsts::{ecdsa, Scalar},
	};
	use futures::{stream::FuturesUnordered, TryStreamExt};
	use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

	#[derive(Clone)]
	struct TestNetworkLayer {
		tx: tokio::sync::broadcast::Sender<Message>,
		rx: Arc<tokio::sync::Mutex<tokio::sync::broadcast::Receiver<Message>>>,
	}

	#[async_trait::async_trait]
	impl NetListen for TestNetworkLayer {
		type Error = frost_signer::net::Error;

		async fn poll(&self, _arg: u32) {}

		async fn next_message(&self) -> Option<Message> {
			dkg_logging::info!(target: "dkg", "Waiting for message");
			let msg = self.rx.lock().await.recv().await.ok();
			dkg_logging::info!(target: "dkg", "Received message");
			msg
		}

		async fn send_message(&self, msg: Message) -> Result<(), Self::Error> {
			dkg_logging::info!(target: "dkg", "Sending message");
			self.tx.send(msg).map(|_| ()).map_err(|_| frost_signer::net::Error::Timeout)?;
			dkg_logging::info!(target: "dkg", "Sent message");
			Ok(())
		}
	}

	fn create_signer_key_ids(signer_id: u32, keys_per_signer: u32) -> Vec<u32> {
		(0..keys_per_signer).map(|i| keys_per_signer * signer_id + i + 1).collect()
	}

	fn create_public_keys(signer_private_keys: &Vec<Scalar>, keys_per_signer: u32) -> PublicKeys {
		let signer_id_keys: HashMap<u32, ecdsa::PublicKey> = signer_private_keys
			.iter()
			.enumerate()
			.map(|(i, key)| ((i + 1) as u32, ecdsa::PublicKey::new(key).unwrap()))
			.collect();

		let key_ids = signer_id_keys
			.iter()
			.flat_map(|(signer_id, signer_key)| {
				(0..keys_per_signer).map(|i| (keys_per_signer * *signer_id - i, signer_key.clone()))
			})
			.collect();

		PublicKeys { signers: signer_id_keys.into_iter().collect(), key_ids }
	}

	#[tokio::test]
	async fn test_dkg() {
		dkg_logging::setup_log();
		let t = 15;
		let n = 5;
		let keys_per_signer = 5;

		let (tx, _) = tokio::sync::broadcast::channel(1000);

		// Generate n+1 pub/priv keys, as well as their network layer
		let mut networks = (1..=(n + 1))
			.into_iter()
			.map(|_idx| {
				let secret_key = Scalar::random(&mut rand::thread_rng());
				let public_key = ecdsa::PublicKey::new(&secret_key).unwrap();
				(
					public_key,
					secret_key,
					TestNetworkLayer {
						tx: tx.clone(),
						rx: tokio::sync::Mutex::new(tx.subscribe()).into(),
					},
				)
			})
			.collect::<Vec<_>>();

		let public_keys = create_public_keys(
			&networks.iter().map(|(_, private_key, _)| private_key.clone()).collect(),
			keys_per_signer,
		);
		let signer_key_ids: HashMap<u32, Vec<u32>> = (0..n)
			.into_iter()
			.map(|i| (i + 1, create_signer_key_ids(i, keys_per_signer)))
			.collect();

		// Generate the coordinator
		let (coordinator_pub_key, coordinator_priv_key, coordinator_network) =
			networks.pop().unwrap();
		let coordinator_config = Config::new(
			t,
			coordinator_pub_key,
			public_keys.clone(),
			signer_key_ids.clone().into_iter().collect(),
			coordinator_priv_key,
			Default::default(),
		);
		let mut coordinator =
			Coordinator::new(0, &coordinator_config, coordinator_network).unwrap();

		dkg_logging::info!(target: "dkg", "Signer key IDs: {:?}", signer_key_ids);
		let nodes = FuturesUnordered::new();

		nodes.push(Box::pin(async move {
			coordinator.run(&Command::Dkg).await.map_err(|err| err.to_string())?;
			Ok::<_, String>(())
		}) as Pin<Box<dyn Future<Output = Result<(), String>> + Send>>);

		// Create a config and coordinator for each node
		for (i, (_public_key, secret_key, network)) in networks.into_iter().enumerate() {
			let signer_config = Config::new(
				t,
				coordinator_pub_key.clone(),
				public_keys.clone(),
				signer_key_ids.clone().into_iter().collect(),
				secret_key,
				Default::default(),
			);
			let mut signer = Signer::new(signer_config, (i + 1) as u32);

			nodes.push(Box::pin(async move {
				signer.start_p2p_async(network).await.map_err(|err| err.to_string())?;
				Ok::<_, String>(())
			}));
		}

		nodes.try_collect::<Vec<_>>().await.unwrap();
	}
}
