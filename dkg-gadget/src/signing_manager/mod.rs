use std::{collections::HashMap, marker::PhantomData};

use dkg_primitives::{UnsignedProposal, MaxProposalLength, types::DKGError};
use sp_core::Get;

use crate::worker::DKGWorker;

use self::work_manager::WorkManager;

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
pub struct SigningManager<B, BE, C, GE> {
    // governs the workload for each node
    work_manager: WorkManager,
    _pd: PhantomData<()>,
}

// 1 unsigned proposal per signing set
const MAX_UNSIGNED_PROPOSALS_PER_SIGNING_SET: usize = 1;

impl<B, BE, C, GE> SigningManager<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities> {

    pub fn new() -> Self {
        Self {
            work_manager: WorkManager::new()
        }
    }

    /// This function is called each time a new block is finalized.
    /// It will then start a signing process for each of the proposals.
    pub fn on_block_finalized(&self, header: &B::Header, dkg_worker: &DKGWorker<B, BE, C, GE>) -> Result<(), DKGError> {
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
				dkg_worker.logger
					.info(format!("🕸️  NOT IN THE SET OF BEST AUTHORITIES: session {session_id}"));
				return Ok(())
			},
		};

		let unsigned_proposals = match dkg_worker.client.runtime_api().get_unsigned_proposals(at) {
			Ok(res) => {
				let mut filtered_unsigned_proposals = Vec::new();
				for proposal in res {
					if let Some(hash) = proposal.hash() {
						if !self.work_manager.currently_signing_proposals.read().contains(&hash) {
							// update unsigned proposal counter
							metric_inc!(self, dkg_unsigned_proposal_counter);
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
				dkg_worker.logger
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
		let mut count = 0;


        for unsigned_proposal in unsigned_proposals {
            /*
                create a seed s where s is keccak256(pk, fN=at, keccak256(unsingedProposal))
                you take this seed and use it as a seed to random number generator.
                generate a t+1 signing set from this RNG
                if we are in this set, we send it to the signing manager, and continue.
                if we are not, we continue the loop.
             */
            let proposal_hash = sp_core::keccak_256(&unsigned_proposal.encode());
            let concat_data = dkg_pub_key.into_iter().chain(at).chain(proposal_hash).collect::<Vec<u8>>();
            let seed = sp_core::keccak_256(concat_data);
            let mut rng = ChaChaRng::from_seed(seed);
        }

		let factorial = |num: u64| match num {
			0 => 1,
			1.. => (1..=num).product(),
		};
		let mut signing_sets = Vec::new();
		let n = factorial(best_authorities.len() as u64);
		let k = factorial((threshold + 1) as u64);
		let n_minus_k = factorial((best_authorities.len() - threshold as usize - 1) as u64);
		let num_combinations = std::cmp::min(n / (k * n_minus_k), MAX_SIGNING_SETS);
		self.logger.debug(format!("Generating {num_combinations} signing sets"));
		while signing_sets.len() < num_combinations as usize {
			if count > 0 {
				seed = sp_core::keccak_256(&seed).to_vec();
			}
			let maybe_set = self.generate_signers(&seed, threshold, best_authorities.clone()).ok();
			if let Some(set) = maybe_set {
				let set = HashSet::<_>::from_iter(set.iter().cloned());
				if !signing_sets.contains(&set) {
					signing_sets.push(set);
				}
			}

			count += 1;
		}
		metric_set!(dkg_worker, dkg_signing_sets, signing_sets.len());

		let mut futures = Vec::with_capacity(signing_sets.len());
		#[allow(clippy::needless_range_loop)]
		for i in 0..signing_sets.len() {
			// Filter for only the signing sets that contain our party index.
			if signing_sets[i].contains(&party_i) {
				dkg_worker.logger.info(format!(
					"🕸️  Session Id {:?} | Async index {:?} | {}-out-of-{} signers: ({:?})",
					session_id,
					i,
					threshold,
					best_authorities.len(),
					signing_sets[i].clone(),
				));
				match self.create_signing_protocol(
					best_authorities.clone(),
					authority_public_key.clone(),
					party_i,
					session_id,
					threshold,
					ProtoStageType::Signing,
					unsigned_proposals.clone(),
					signing_sets[i].clone().into_iter().sorted().collect::<Vec<_>>(),
					// using i here as the async index is not correct at all,
					// instead we should find a free index in the `signing_rounds` and use that
					//
					// FIXME: use a free index in the `signing_rounds` instead of `i`
					i as _,
				) {
					Ok(task) => futures.push(task),
					Err(err) => {
						dkg_worker.logger.error(format!("Error creating signing protocol: {:?}", &err));
						dkg_worker.handle_dkg_error(err)
					},
				}
			}
		}

		if futures.is_empty() {
			dkg_worker.logger
				.error("While creating the signing protocol, 0 were created".to_string());
			Err(DKGError::GenericError {
				reason: "While creating the signing protocol, 0 were created".to_string(),
			})
		} else {
			let proposal_hashes =
				unsigned_proposals.iter().filter_map(|x| x.hash()).collect::<Vec<_>>();
			// save the proposal hashes in the currently_signing_proposals.
			// this is used to check if we have already signed a proposal or not.
            // TODO! send task to work manager
			let logger = dkg_worker.logger.clone();
			self.currently_signing_proposals.write().extend(proposal_hashes.clone());
			logger.info(format!("Signing protocol created, added {:?} proposals to currently_signing_proposals list", proposal_hashes.len()));
			// the goal of the meta task is to select the first winner
			let meta_signing_protocol = async move {
				// select the first future to return Ok(()), ignoring every failure
				// (note: the errors are not truly ignored since each individual future
				// has logic to handle errors internally, including misbehaviour monitors
				let mut results = futures::future::select_ok(futures).await.into_iter();
				if let Some((_success, _losing_futures)) = results.next() {
					logger.info(format!(
						"*** SUCCESSFULLY EXECUTED meta signing protocol {_success:?} ***"
					));
				} else {
					logger.warn("*** UNSUCCESSFULLY EXECUTED meta signing protocol".to_string());
				}
			};

			// spawn in parallel
			let _handle = tokio::task::spawn(meta_signing_protocol);
			Ok(())
		}
    }


    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
	#[cfg_attr(
		feature = "debug-tracing",
		dkg_logging::instrument(
			target = "dkg",
			skip_all,
			err,
			fields(session_id, threshold, stage, async_index)
		)
	)]
	fn create_signing_protocol(
		&self,
        dkg_worker: &DKGWorker,
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
		unsigned_proposals: Vec<UnsignedProposal<MaxProposalLength>>,
		signing_set: Vec<KeygenPartyId>,
		async_index: u8,
	) -> Result<Pin<Box<dyn Future<Output = Result<u8, DKGError>> + Send + 'static>>, DKGError> {
		let async_proto_params = dkg_worker.generate_async_proto_params(
			best_authorities,
			authority_public_key,
			party_i,
			session_id,
			stage,
			async_index,
			crate::DKG_SIGNING_PROTOCOL_NAME,
		)?;

		let err_handler_tx = dkg_worker.error_handler.clone();
		let proposal_hashes =
			unsigned_proposals.iter().filter_map(|p| p.hash()).collect::<Vec<_>>();
		let meta_handler = GenericAsyncHandler::setup_signing(
			async_proto_params,
			threshold,
			unsigned_proposals,
			signing_set,
			async_index,
		)?;
		let logger = dkg_worker.logger.clone();
		let work_manager = self.work_manager.clone();
		let task = async move {
			match meta_handler.await {
				Ok(_) => {
					logger.info("The meta handler has executed successfully".to_string());
					Ok(async_index)
				},

				Err(err) => {
					logger.error(format!("Error executing meta handler {:?}", &err));
					let _ = err_handler_tx.send(err.clone());
					// remove proposal hashes, so that they can be reprocessed
					let mut lock = work_manager.currently_signing_proposals.write();
					proposal_hashes.iter().for_each(|h| {
						lock.remove(h);
					});
					logger.info(format!(
						"Removed {:?} proposal hashes from currently signing queue",
						proposal_hashes.len()
					));
					Err(err)
				},
			}
		};

		Ok(Box::pin(task))
	}
}