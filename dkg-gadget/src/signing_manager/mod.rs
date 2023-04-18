use std::marker::PhantomData;

use dkg_primitives::{types::DKGError, MaxProposalLength, UnsignedProposal};

use self::work_manager::WorkManager;
use crate::{
	async_protocols::{remote::AsyncProtocolRemote, GenericAsyncHandler, KeygenPartyId},
	gossip_engine::GossipEngineIface,
	metric_inc,
	utils::SendFuture,
	worker::{DKGWorker, KeystoreExt, ProtoStageType},
	*,
};
use codec::Encode;
use dkg_primitives::{utils::select_random_set, SessionId};
use dkg_runtime_primitives::crypto::Public;
use sp_api::HeaderT;
use std::pin::Pin;

/// For balancing the amount of work done by each node
pub mod work_manager;

/// The signing manager is triggered each time a new block is finalized.
/// It will then start a signing process for each of the proposals. SigningManagerV2 uses
/// 1 signing set per proposal for simplicity.
///
/// The steps:
/// Fetch the current DKG PublicKey pk
/// Fetch all the Unsigned Proposals from on-chain unsignedProposals
/// for each unsinged proposal unsingedProposal do the following:
/// create a seed s where s is keccak256(pk, fN, keccak256(unsingedProposal))
/// you take this seed and use it as a seed to random number generator.
/// generate a t+1 signing set from this RNG
/// if we are in this set, we send it to the work manager, and continue.
/// if we are not, we continue the loop.
pub struct SigningManager<B: Block, BE, C, GE> {
	// governs the workload for each node
	work_manager: WorkManager<B>,
	_pd: PhantomData<(B, BE, C, GE)>,
}

impl<B: Block, BE, C, GE> Clone for SigningManager<B, BE, C, GE> {
	fn clone(&self) -> Self {
		Self { work_manager: self.work_manager.clone(), _pd: self._pd }
	}
}

// 1 unsigned proposal per signing set
const MAX_UNSIGNED_PROPOSALS_PER_SIGNING_SET: usize = 1;

impl<B, BE, C, GE> SigningManager<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	pub fn new(logger: DebugLogger) -> Self {
		Self { work_manager: WorkManager::<B>::new(logger, 5), _pd: Default::default() }
	}

	/// This function is called each time a new block is finalized.
	/// It will then start a signing process for each of the proposals.
	pub fn on_block_finalized(
		&self,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) -> Result<(), DKGError> {
		let on_chain_dkg = dkg_worker.get_dkg_pub_key(header);
		let session_id = on_chain_dkg.0;
		let dkg_pub_key = on_chain_dkg.1;
		let at = header.hash();
		// Check whether the worker is in the best set or return
		let party_i = match dkg_worker.get_party_index(header) {
			Some(party_index) => {
				dkg_worker.logger.info(format!("🕸️  PARTY {party_index} | SESSION {session_id} | IN THE SET OF BEST AUTHORITIES"));
				KeygenPartyId::try_from(party_index)?
			},
			None => {
				dkg_worker
					.logger
					.info(format!("🕸️  NOT IN THE SET OF BEST AUTHORITIES: session {session_id}"));
				return Ok(())
			},
		};

		let unsigned_proposals = match dkg_worker.client.runtime_api().get_unsigned_proposals(at) {
			Ok(res) => {
				let mut filtered_unsigned_proposals = Vec::new();
				for proposal in res {
					if let Some(hash) = proposal.hash() {
						if !self.work_manager.job_exists(&hash) {
							// update unsigned proposal counter
							metric_inc!(dkg_worker, dkg_unsigned_proposal_counter);
							filtered_unsigned_proposals.push(proposal);
						}

						// lets limit the max proposals we sign at one time to prevent overflow
						if filtered_unsigned_proposals.len() >=
							MAX_UNSIGNED_PROPOSALS_PER_SIGNING_SET
						{
							break
						}
					}
				}
				filtered_unsigned_proposals
			},
			Err(e) => {
				dkg_worker
					.logger
					.error(format!("🕸️  PARTY {party_i} | Failed to get unsigned proposals: {e:?}"));
				return Err(DKGError::GenericError {
					reason: format!("Failed to get unsigned proposals: {e:?}"),
				})
			},
		};
		if unsigned_proposals.is_empty() {
			return Ok(())
		} else {
			dkg_worker.logger.debug(format!(
				"🕸️  PARTY {party_i} | Got unsigned proposals count {}",
				unsigned_proposals.len()
			));
		}

		let best_authorities: Vec<_> = dkg_worker
			.get_best_authorities(header)
			.into_iter()
			.flat_map(|(i, p)| KeygenPartyId::try_from(i).map(|i| (i, p)))
			.collect();
		let threshold = dkg_worker.get_signature_threshold(header);
		let authority_public_key = dkg_worker.get_authority_public_key();

		for unsigned_proposal in unsigned_proposals {
			/*
			   create a seed s where s is keccak256(pk, fN=at, keccak256(unsingedProposal))
			   you take this seed and use it as a seed to random number generator.
			   generate a t+1 signing set from this RNG
			   if we are in this set, we send it to the signing manager, and continue.
			   if we are not, we continue the loop.
			*/
			let unsigned_proposal_hash = sp_core::keccak_256(&unsigned_proposal.encode());
			let concat_data = dkg_pub_key
				.clone()
				.into_iter()
				.chain(at.encode())
				.chain(unsigned_proposal_hash)
				.collect::<Vec<u8>>();
			let seed = sp_core::keccak_256(&concat_data);

			let maybe_set = self
				.generate_signers(&seed, threshold, best_authorities.clone(), dkg_worker)
				.ok();
			if let Some(signing_set) = maybe_set {
				// if we are in the set, send to work manager
				if signing_set.contains(&party_i) {
					dkg_worker.logger.info(format!(
						"🕸️  Session Id {:?} | {}-out-of-{} signers: ({:?})",
						session_id,
						threshold,
						best_authorities.len(),
						signing_set,
					));
					match self.create_signing_protocol(
						dkg_worker,
						best_authorities.clone(),
						authority_public_key.clone(),
						party_i,
						session_id,
						threshold,
						ProtoStageType::Signing { unsigned_proposal_hash },
						unsigned_proposal,
						signing_set,
					) {
						Ok((handle, task)) => {
							// send task to the work manager
							self.work_manager.push_task(unsigned_proposal_hash, handle, task)?;
						},
						Err(err) => {
							dkg_worker
								.logger
								.error(format!("Error creating signing protocol: {:?}", &err));
							dkg_worker.handle_dkg_error(err.clone());
							return Err(err)
						},
					}
				}
			}
		}

		Ok(())
	}

	#[allow(clippy::too_many_arguments, clippy::type_complexity)]
	#[cfg_attr(
		feature = "debug-tracing",
		dkg_logging::instrument(
			target = "dkg",
			skip_all,
			err,
			fields(session_id, threshold, stage, party_i)
		)
	)]
	fn create_signing_protocol(
		&self,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
		unsigned_proposal: UnsignedProposal<MaxProposalLength>,
		signing_set: Vec<KeygenPartyId>,
	) -> Result<(AsyncProtocolRemote<NumberFor<B>>, Pin<Box<dyn SendFuture<'static, ()>>>), DKGError>
	{
		let async_proto_params = dkg_worker.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			stage,
			crate::DKG_SIGNING_PROTOCOL_NAME,
		)?;

		let handle = async_proto_params.handle.clone();

		let err_handler_tx = dkg_worker.error_handler.clone();
		let meta_handler = GenericAsyncHandler::setup_signing(
			async_proto_params,
			threshold,
			unsigned_proposal,
			signing_set,
		)?;
		let logger = dkg_worker.logger.clone();
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
	}

	/// After keygen, this should be called to generate a random set of signers
	/// NOTE: since the random set is called using a deterministic seed to and RNG,
	/// the resulting set is deterministic
	fn generate_signers(
		&self,
		seed: &[u8],
		t: u16,
		best_authorities: Vec<(KeygenPartyId, Public)>,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) -> Result<Vec<KeygenPartyId>, DKGError> {
		let only_public_keys = best_authorities.iter().map(|(_, p)| p).cloned().collect::<Vec<_>>();
		let mut final_set = dkg_worker.get_unjailed_signers(&only_public_keys)?;
		// Mutate the final set if we don't have enough unjailed signers
		if final_set.len() <= t as usize {
			let jailed_set = dkg_worker.get_jailed_signers(&only_public_keys)?;
			let diff = t as usize + 1 - final_set.len();
			final_set = final_set
				.iter()
				.chain(jailed_set.iter().take(diff))
				.cloned()
				.collect::<Vec<_>>();
		}

		select_random_set(seed, final_set, t + 1)
			.map(|set| set.into_iter().flat_map(KeygenPartyId::try_from).collect::<Vec<_>>())
			.map_err(|err| DKGError::CreateOfflineStage {
				reason: format!("generate_signers failed, reason: {err}"),
			})
	}
}
