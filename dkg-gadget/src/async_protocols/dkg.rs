use crate::{
	async_protocols::{
		remote::AsyncProtocolRemote, GenericAsyncHandler, KeygenPartyId, KeygenRound,
	},
	gossip_engine::GossipEngineIface,
	keygen_manager::KeygenState,
	utils::SendFuture,
	worker::{DKGWorker, ProtoStageType},
	Client,
};
use async_trait::async_trait;
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	BatchId, DKGApi, MaxAuthorities, MaxProposalLength, MaxProposalsInBatch, SessionId,
	StoredUnsignedProposalBatch,
};
use parking_lot::RwLock;
use sc_client_api::Backend;
use sp_runtime::traits::{Block, NumberFor};
use std::{
	marker::PhantomData,
	pin::Pin,
	sync::{atomic::Ordering, Arc},
};

pub enum DKGProtocolSetupParameters<B: Block> {
	MpEcdsa {
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		associated_block: NumberFor<B>,
		threshold: u16,
		stage: ProtoStageType,
		keygen_protocol_hash: [u8; 32],
	},
	WTFrost {},
}

pub enum SigningProtocolSetupParameters<B: Block> {
	MpEcdsa {
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BatchId,
			MaxProposalLength,
			MaxProposalsInBatch,
			NumberFor<B>,
		>,
		signing_set: Vec<KeygenPartyId>,
		associated_block_id: NumberFor<B>,
	},
	WTFrost {},
}

/// A type which is used directly by the Job Manager to initialize and manage the DKG protocol
pub type ProtocolInitReturn<B> =
	(AsyncProtocolRemote<NumberFor<B>>, Pin<Box<dyn SendFuture<'static, ()>>>);

#[async_trait]
/// Generalizes the DKGWorker::initialize_keygen_protocol and DKGWorker::initialize_signing_protocol
/// Also includes two functions which are used for determining whether a DKG can handle the request
/// parameters, which is used in the [`DKGModules`] implementation
pub trait DKG<B: Block>: Send + Sync {
	async fn initialize_keygen_protocol(
		&self,
		params: DKGProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>>;
	async fn initialize_signing_protocol(
		&self,
		params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError>;
	fn can_handle_keygen_request(&self, params: &DKGProtocolSetupParameters<B>) -> bool;
	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool;
}

pub struct MpEcdsaDKG<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
{
	dkg_worker: DKGWorker<B, BE, C, GE>,
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
		params: DKGProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		if let DKGProtocolSetupParameters::MpEcdsa {
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
			match self.dkg_worker.generate_async_proto_params(
				best_authorities,
				authority_public_key,
				party_i,
				session_id,
				stage,
				crate::DKG_KEYGEN_PROTOCOL_NAME,
				associated_block,
			) {
				Ok(async_proto_params) => {
					let err_handler_tx = self.dkg_worker.error_handler.clone();

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
			)?;

			let handle = async_proto_params.handle.clone();

			let err_handler_tx = self.dkg_worker.error_handler.clone();
			let meta_handler = GenericAsyncHandler::setup_signing(
				async_proto_params,
				threshold,
				unsigned_proposal_batch,
				signing_set,
			)?;
			let logger = self.dkg_worker.logger.clone();
			let task = async move {
				match meta_handler.await {
					Ok(_) => {
						logger.info("The meta handler has executed successfully".to_string());
						Ok(())
					},

					Err(err) => {
						logger.error(format!("Error executing meta handler {:?}", &err));
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

	fn can_handle_keygen_request(&self, params: &DKGProtocolSetupParameters<B>) -> bool {
		matches!(params, DKGProtocolSetupParameters::MpEcdsa { .. })
	}

	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool {
		matches!(params, SigningProtocolSetupParameters::MpEcdsa { .. })
	}
}

pub struct WTFrostDKG {}

#[async_trait]
impl<B: Block> DKG<B> for WTFrostDKG {
	async fn initialize_keygen_protocol(
		&self,
		_params: DKGProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		todo!()
	}

	async fn initialize_signing_protocol(
		&self,
		_params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError> {
		todo!()
	}

	fn can_handle_keygen_request(&self, params: &DKGProtocolSetupParameters<B>) -> bool {
		matches!(params, DKGProtocolSetupParameters::WTFrost { .. })
	}

	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool {
		matches!(params, SigningProtocolSetupParameters::WTFrost { .. })
	}
}

/// Holds a list of DKGs that can be used at runtime
pub struct DKGModules<B: Block, BE, C, GE> {
	dkgs: Arc<RwLock<Vec<Arc<dyn DKG<B>>>>>,
	_pd: PhantomData<(BE, C, GE)>,
}

impl<B, BE, C, GE> DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	pub fn initialize(&self, dkg_worker: DKGWorker<B, BE, C, GE>) {
		*self.dkgs.write() = vec![Arc::new(MpEcdsaDKG { dkg_worker }), Arc::new(WTFrostDKG {})]
	}

	pub fn get_keygen_protocol(
		&self,
		params: &DKGProtocolSetupParameters<B>,
	) -> Option<Arc<dyn DKG<B>>> {
		self.dkgs
			.read()
			.iter()
			.find(|dkg| dkg.can_handle_keygen_request(params))
			.cloned()
	}

	pub fn get_signing_protocol(
		&self,
		params: &SigningProtocolSetupParameters<B>,
	) -> Option<Arc<dyn DKG<B>>> {
		self.dkgs
			.read()
			.iter()
			.find(|dkg| dkg.can_handle_signing_request(params))
			.cloned()
	}
}

// implement Default for DKGModules
impl<B, BE, C, GE> Default for DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
{
	fn default() -> Self {
		Self { dkgs: Arc::new(RwLock::new(vec![])), _pd: PhantomData }
	}
}

impl<B, BE, C, GE> Clone for DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
{
	fn clone(&self) -> Self {
		Self { dkgs: self.dkgs.clone(), _pd: PhantomData }
	}
}
