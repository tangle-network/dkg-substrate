use crate::{
	async_protocols::{GenericAsyncHandler, KeygenRound},
	dkg_modules::{
		KeygenProtocolSetupParameters, ProtocolInitReturn, SigningProtocolSetupParameters, DKG,
	},
	gossip_engine::GossipEngineIface,
	keygen_manager::KeygenState,
	signing_manager::SigningResult,
	worker::{DKGWorker, ProtoStageType},
	Client,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, SSID};
use dkg_runtime_primitives::{crypto::AuthorityId, DKGApi, MaxAuthorities, MaxProposalLength};
use sc_client_api::Backend;
use sp_runtime::traits::{Block, NumberFor};
use std::sync::atomic::Ordering;

/// DKG module for Multi-Party ECDSA
pub struct MpEcdsaDKG<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	pub(super) dkg_worker: DKGWorker<B, BE, C, GE>,
}

#[async_trait]
impl<B, BE, C, GE> DKG<B> for MpEcdsaDKG<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	async fn initialize_keygen_protocol(
		&self,
		params: KeygenProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		if let KeygenProtocolSetupParameters::MpEcdsa {
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			associated_block,
			threshold,
			stage,
			keygen_protocol_hash,
		} = params
		{
			const KEYGEN_SSID: SSID = 0;
			match self.dkg_worker.generate_async_proto_params(
				best_authorities,
				authority_public_key,
				party_i,
				session_id,
				stage,
				crate::DKG_KEYGEN_PROTOCOL_NAME,
				associated_block,
				KEYGEN_SSID,
			) {
				Ok(async_proto_params) => {
					let err_handler_tx = self.dkg_worker.error_handler_channel.tx.clone();

					let remote = async_proto_params.handle.clone();
					let keygen_manager = self.dkg_worker.keygen_manager.clone();
					let status = match stage {
						ProtoStageType::KeygenGenesis => KeygenRound::Genesis,
						ProtoStageType::KeygenStandard => KeygenRound::Next,
						ProtoStageType::Signing { .. } => {
							unreachable!("Should not happen here")
						},
					};

					match GenericAsyncHandler::setup_keygen(
						async_proto_params,
						threshold,
						status,
						keygen_protocol_hash,
					) {
						Ok(meta_handler) => {
							let logger = self.dkg_worker.logger.clone();
							let signing_manager = self.dkg_worker.signing_manager.clone();
							signing_manager.keygen_lock();
							let task = async move {
								match meta_handler.await {
									Ok(_) => {
										keygen_manager.set_state(KeygenState::KeygenCompleted {
											session_completed: session_id,
										});
										let _ = keygen_manager
											.finished_count
											.fetch_add(1, Ordering::SeqCst);
										signing_manager.keygen_unlock();
										logger.info(
											"The keygen meta handler has executed successfully"
												.to_string(),
										);

										Ok(())
									},

									Err(err) => {
										logger.error(format!(
											"Error executing meta handler {:?}",
											&err
										));
										keygen_manager
											.set_state(KeygenState::Failed { session_id });
										signing_manager.keygen_unlock();
										let _ = err_handler_tx.send(err.clone());
										Err(err)
									},
								}
							};

							self.dkg_worker.logger.debug(format!("Created Keygen Protocol task for session {session_id} with status {status:?}"));
							return Some((remote, Box::pin(task)))
						},

						Err(err) => {
							self.dkg_worker
								.logger
								.error(format!("Error starting meta handler {:?}", &err));
							self.dkg_worker.handle_dkg_error(err).await;
						},
					}
				},

				Err(err) => {
					self.dkg_worker.handle_dkg_error(err).await;
				},
			}

			None
		} else {
			unreachable!("Should not happen (keygen)")
		}
	}

	async fn initialize_signing_protocol(
		&self,
		params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError> {
		if let SigningProtocolSetupParameters::MpEcdsa {
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			threshold,
			stage,
			unsigned_proposal_batch,
			signing_set,
			associated_block_id,
			ssid,
			unsigned_proposal_hash,
		} = params
		{
			self.dkg_worker.logger.debug(format!("{party_i:?} All Parameters: {best_authorities:?} | authority_pub_key: {authority_public_key:?} | session_id: {session_id:?} | threshold: {threshold:?} | stage: {stage:?} | unsigned_proposal_batch: {unsigned_proposal_batch:?} | signing_set: {signing_set:?} | associated_block_id: {associated_block_id:?}"));
			let async_proto_params = self.dkg_worker.generate_async_proto_params(
				best_authorities,
				authority_public_key,
				party_i,
				session_id,
				stage,
				crate::DKG_SIGNING_PROTOCOL_NAME,
				associated_block_id,
				ssid,
			)?;

			let handle = async_proto_params.handle.clone();

			let err_handler_tx = self.dkg_worker.error_handler_channel.tx.clone();
			let meta_handler = GenericAsyncHandler::setup_signing(
				async_proto_params,
				threshold,
				unsigned_proposal_batch,
				signing_set,
			)?;
			let logger = self.dkg_worker.logger.clone();
			let signing_manager = self.dkg_worker.signing_manager.clone();
			let task = async move {
				match meta_handler.await {
					Ok(_) => {
						logger.info("The meta handler has executed successfully");
						signing_manager
							.update_local_signing_set_state(SigningResult::Success {
								unsigned_proposal_hash,
							})
							.await;
						Ok(())
					},

					Err(err) => {
						logger.error(format!("Error executing meta handler {:?}", &err));
						signing_manager
							.update_local_signing_set_state(SigningResult::Failure {
								unsigned_proposal_hash,
								ssid,
							})
							.await;
						let _ = err_handler_tx.send(err.clone());
						Err(err)
					},
				}
			};

			Ok((handle, Box::pin(task)))
		} else {
			unreachable!("Should not happen (signing)")
		}
	}

	fn can_handle_keygen_request(&self, params: &KeygenProtocolSetupParameters<B>) -> bool {
		matches!(params, KeygenProtocolSetupParameters::MpEcdsa { .. })
	}

	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool {
		matches!(params, SigningProtocolSetupParameters::MpEcdsa { .. })
	}
}
