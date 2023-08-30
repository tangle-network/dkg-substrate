use std::{
	collections::{HashMap, HashSet},
	hash::{Hash, Hasher},
	marker::PhantomData,
};

use dkg_primitives::{
	types::{DKGError, SignedDKGMessage},
	MaxProposalLength,
};

use self::work_manager::WorkManager;
use crate::{
	async_protocols::KeygenPartyId,
	constants::signing_manager::*,
	dkg_modules::SigningProtocolSetupParameters,
	gossip_engine::GossipEngineIface,
	metric_inc,
	signing_manager::work_manager::PollMethod,
	worker::{DKGWorker, HasLatestHeader, KeystoreExt, ProtoStageType},
	*,
};
use codec::Encode;
use dkg_primitives::{types::SSID, utils::select_random_set};
use dkg_runtime_primitives::{
	crypto::Public, BatchId, MaxProposalsInBatch, SessionId, StoredUnsignedProposalBatch,
	SIGN_TIMEOUT,
};
use itertools::Itertools;
use sp_api::HeaderT;
use sp_arithmetic::traits::SaturatedConversion;
use sp_runtime::traits::Header;
use std::sync::atomic::{AtomicBool, Ordering};
use webb_proposals::TypedChainId;

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
	// Governs the workload for each node
	work_manager: WorkManager<B>,
	lock: Arc<AtomicBool>,
	// A map that keeps track of each signing set used for a given proposal hash
	ssid_history: Arc<tokio::sync::Mutex<HashMap<[u8; 32], SigningSetHistoryTracker>>>,
	_pd: PhantomData<(B, BE, C, GE)>,
}

#[derive(Default)]
struct SigningSetHistoryTracker {
	ssids_attempted: HashSet<SSID>,
	attempted_sets: HashSet<ProposedSigningSet>,
}

type UnsignedProposalsVec<B> =
	Vec<StoredUnsignedProposalBatch<BatchId, MaxProposalLength, MaxProposalsInBatch, B>>;

impl<B: Block, BE, C, GE> Clone for SigningManager<B, BE, C, GE> {
	fn clone(&self) -> Self {
		Self {
			work_manager: self.work_manager.clone(),
			_pd: self._pd,
			lock: self.lock.clone(),
			ssid_history: self.ssid_history.clone(),
		}
	}
}

impl<B, BE, C, GE> SigningManager<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	GE: GossipEngineIface + 'static,
	C: Client<B, BE> + 'static,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	pub fn new(logger: DebugLogger, clock: impl HasLatestHeader<B>) -> Self {
		Self {
			work_manager: WorkManager::<B>::new(
				logger,
				clock,
				MAX_RUNNING_TASKS,
				MAX_ENQUEUED_TASKS,
				PollMethod::Interval { millis: JOB_POLL_INTERVAL_IN_MILLISECONDS },
			),
			ssid_history: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
			lock: Arc::new(AtomicBool::new(false)),
			_pd: Default::default(),
		}
	}

	pub fn deliver_message(&self, message: SignedDKGMessage<Public>) {
		let message_task_hash =
			*message.msg.payload.unsigned_proposal_hash().expect("Bad message type");
		self.work_manager.deliver_message(message, message_task_hash)
	}

	pub fn clear_enqueued_proposal_tasks(&self) {
		self.work_manager.clear_enqueued_tasks();
	}

	// prevents on_block_finalized from executing
	pub fn keygen_lock(&self) {
		self.lock.store(true, Ordering::SeqCst);
	}

	// allows the on_block_finalized task to be executed
	pub fn keygen_unlock(&self) {
		self.lock.store(false, Ordering::SeqCst);
	}

	/// This function is called each time a new block is finalized.
	/// It will then start a signing process for each of the proposals.
	#[allow(clippy::let_underscore_future)]
	pub async fn on_block_finalized(
		&self,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) -> Result<(), DKGError> {
		let header = &header;
		let dkg_worker = &dkg_worker;
		// if a keygen is running, don't start any new signing tasks
		// until later
		if self.lock.load(Ordering::SeqCst) {
			dkg_worker
				.logger
				.debug("Will skip handling block finalized event because keygen is running");
			return Ok(())
		}

		let on_chain_dkg = dkg_worker.get_dkg_pub_key(header).await;
		let session_id = on_chain_dkg.0;
		let dkg_pub_key = on_chain_dkg.1;
		let now: u64 = (*header.number()).saturated_into();

		// Check whether the worker is in the best set or return
		let party_i = match dkg_worker.get_party_index(header).await {
			Some(party_index) => {
				dkg_worker.logger.info(format!("🕸️  SIGNING PARTY {party_index} | SESSION {session_id} | IN THE SET OF BEST AUTHORITIES"));
				KeygenPartyId::try_from(party_index)?
			},
			None => {
				dkg_worker
					.logger
					.info(format!("🕸️  NOT IN THE SET OF BEST AUTHORITIES: session {session_id}"));
				return Ok(())
			},
		};

		dkg_worker.logger.info_signing("About to get unsigned proposals ...");

		let (unsigned_proposals, full_unsigned_proposals) =
			self.get_unsigned_proposals(header, dkg_worker, party_i).await?;

		if unsigned_proposals.is_empty() {
			dkg_worker.logger.info("No unsigned proposals to sign");
			return Ok(())
		} else {
			dkg_worker.logger.debug(format!(
				"🕸️  PARTY {party_i} | Got unsigned proposals count {}",
				unsigned_proposals.len()
			));
		}

		let best_authorities: Vec<_> = dkg_worker
			.get_best_authorities(header)
			.await
			.into_iter()
			.flat_map(|(i, p)| KeygenPartyId::try_from(i).map(|i| (i, p)))
			.collect();

		let threshold = dkg_worker.get_signature_threshold(header).await;
		let authority_public_key = dkg_worker.get_authority_public_key();

		let mut ssid_lock = self.ssid_history.lock().await;

		for batch in unsigned_proposals {
			dkg_worker.logger.info("Checking batch ...");
			let typed_chain_id = batch.proposals.first().expect("Empty batch!").typed_chain_id;
			if !self.work_manager.can_submit_more_tasks() {
				dkg_worker.logger.info(
					"Will not submit more unsigned proposals because the work manager is full",
				);
				break
			}

			let unsigned_proposal_bytes = batch.encode();
			let unsigned_proposal_hash = batch.hash().expect("unable to hash proposal");

			// First, check to see if the proposal is already done. If it is, skip it
			if !full_unsigned_proposals.iter().any(|p| p.hash() == Some(unsigned_proposal_hash)) {
				dkg_worker
					.logger
					.info("Since this proposal is already done, we will move on to the next");
				ssid_lock.remove(&unsigned_proposal_hash);
				continue
			}

			let latest_attempt = ssid_lock
				.entry(unsigned_proposal_hash)
				.or_default()
				.attempted_sets
				.iter()
				.map(|r| r.locally_proposed_at)
				.max();

			// Next, check to see if we are already locally waiting for this set to complete
			if let Some(init_time) = latest_attempt {
				let stalled = now >= init_time + SIGN_TIMEOUT as u64;
				if stalled {
					dkg_worker.logger.info(
						"Since the latest attempt of this proposal is stalled, we will proceed",
					)
				} else {
					dkg_worker.logger.info("Since the latest attempt of this proposal is not stalled, we will skip this proposal");
					continue
				}
			}

			// Either one of two cases are possible here
			// * We have never attempted to sign this proposal before
			// * We have attempted to sign this proposal before, but, it is stalled and we need to
			//   start the next one
			// In either case, we need to find the next UNIQUE signing set to attempt

			// Loop until we generate a signing set that is unique compared to previous signing sets
			for _ in 0..MAX_POTENTIAL_RETRIES_PER_UNSIGNED_PROPOSAL {
				if ssid_lock.entry(unsigned_proposal_hash).or_default().ssids_attempted.len() >=
					MAX_POTENTIAL_RETRIES_PER_UNSIGNED_PROPOSAL as usize
				{
					dkg_worker.logger.info(format!("🕸️  PARTY {party_i} | SESSION {session_id} | MAX_POTENTIAL_RETRIES_PER_UNSIGNED_PROPOSAL reached. Will not attempt to sign this proposal"));
					break
				}

				// First, generate the next SSID
				let next_ssid = ssid_lock
					.entry(unsigned_proposal_hash)
					.or_default()
					.ssids_attempted
					.iter()
					.max()
					.copied()
					.map(|r| r.wrapping_add(1)) // Take the max value and add one to it to get the next SSID
					.unwrap_or(0); // If there are no elements, start off at SSID 0

				// Immediately add the next SSID to the ssids_attempted, that way,
				// if we don't later generate a novel set, we start at the next index
				ssid_lock
					.entry(unsigned_proposal_hash)
					.or_default()
					.ssids_attempted
					.insert(next_ssid);

				if let Some(signing_set) = self
					.maybe_create_signing_set(
						dkg_worker,
						&dkg_pub_key,
						&unsigned_proposal_bytes,
						next_ssid,
						threshold,
						session_id,
						&best_authorities,
						header,
					)
					.await
				{
					let ssid = next_ssid;
					let local_in_this_set = signing_set.signing_set.contains(&party_i);

					// Now, check to see if this signing set is novel. If it is, we can break out of
					// the loop
					let is_novel = !ssid_lock
						.entry(unsigned_proposal_hash)
						.or_default()
						.attempted_sets
						.iter()
						.any(|set| set.signing_set == signing_set.signing_set);

					// If this set it novel, and, we are NOT in this set, let the other nodes
					// attempt this protocol. Break out of the loop
					if is_novel && !local_in_this_set {
						// TODO: before trying the NEXT unique signing set the next time a finality
						// notification happens, check to see if the full_unsigned_proposals
						// contains this proposal. If it does, AND, we know it is stalled,
						// we can try running again. If it does contain the proposal, but it's NOT
						// stalled, we return immediately and don't run protocols for this unsigned
						// hash until we know it is stalled OR if the protocol was a success. We can
						// tell a protocol is a success if this unsigned proposal hash is not inside
						// the full_unsigned_proposals anymore
						dkg_worker.logger.debug(format!("Attempted SSID {ssid}={signing_set:?}, however, this set is novel and local is not in this set. Delegating to other nodes"));
						// To ensure we count this the next time the delegated nodes fail, add this
						// to the list
						ssid_lock
							.entry(unsigned_proposal_hash)
							.or_default()
							.attempted_sets
							.insert(signing_set.clone());
						break
					}

					// If this set is not novel, we must continue looping until we find a novel set
					if !is_novel {
						dkg_worker.logger.debug(format!("Attempted SSID {ssid}={signing_set:?}, however, this set is not novel. Continuing to loop"));
						continue
					}

					// If we reach here, then, this signing set is NOVEL and we are in this set.
					// Add this to the attempted sets
					ssid_lock
						.entry(unsigned_proposal_hash)
						.or_default()
						.attempted_sets
						.insert(signing_set.clone());

					dkg_worker.logger.info(format!(
						"🕸️  Session Id {:?} | SSID {} | {}-out-of-{} signers: ({:?})",
						session_id,
						ssid,
						threshold,
						signing_set.signing_set.len(),
						signing_set,
					));

					let params = SigningProtocolSetupParameters::MpEcdsa {
						best_authorities: best_authorities.clone(),
						authority_public_key: authority_public_key.clone(),
						party_i,
						session_id,
						threshold,
						stage: ProtoStageType::Signing { unsigned_proposal_hash },
						unsigned_proposal_batch: batch.clone(),
						signing_set: signing_set.into_sorted_vec(),
						associated_block_id: *header.number(),
						ssid,
						unsigned_proposal_hash,
					};

					let signing_protocol = dkg_worker
						.dkg_modules
						.get_signing_protocol(&params)
						.expect("Standard signing protocol should exist");
					match signing_protocol.initialize_signing_protocol(params).await {
						Ok((handle, task)) => {
							// Send task to the work manager. Force start if the type chain ID
							// is None, implying this is a proposal needed for rotating sessions
							// and thus a priority
							let force_start = typed_chain_id == TypedChainId::None;
							self.work_manager.push_task(
								unsigned_proposal_hash,
								force_start,
								handle,
								task,
							)?;
							// Break from the inner loop to continue to the next unsigned proposal
							break
						},
						Err(err) => {
							dkg_worker
								.logger
								.error(format!("Error creating signing protocol: {:?}", &err));
							dkg_worker.handle_dkg_error(err.clone()).await;
							return Err(err)
						},
					}
				} else {
					// If we get None, break out of the loop unconditionally
					break
				}
			}
		}

		Ok(())
	}

	/// Whenever a signing protocol finishes, it MUST call this function
	pub async fn update_local_signing_set_state(&self, update: SigningResult) {
		let mut lock = self.ssid_history.lock().await;
		match update {
			SigningResult::Success { unsigned_proposal_hash } => {
				// On success, remove the signing set history for this unsigned proposal hash
				lock.remove(&unsigned_proposal_hash);
			},
			SigningResult::Failure { unsigned_proposal_hash, ssid } => {
				lock.entry(unsigned_proposal_hash).or_default().ssids_attempted.insert(ssid);
			},
		}
	}

	async fn get_unsigned_proposals(
		&self,
		header: &B::Header,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		party_i: KeygenPartyId,
	) -> Result<
		(
			UnsignedProposalsVec<<<B as Block>::Header as Header>::Number>,
			UnsignedProposalsVec<<<B as Block>::Header as Header>::Number>,
		),
		DKGError,
	> {
		match dkg_worker.get_unsigned_proposal_batches(header).await {
			Ok(mut all_proposals) => {
				// sort proposals by timestamp, we want to pick the oldest proposal to sign
				all_proposals.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
				let mut filtered_unsigned_proposals = Vec::new();
				for proposal in &all_proposals {
					if let Some(hash) = proposal.hash() {
						// only submit the job if it isn't already running
						if !self.work_manager.job_exists(&hash) {
							// update unsigned proposal counter
							metric_inc!(dkg_worker, dkg_unsigned_proposal_counter);
							filtered_unsigned_proposals.push(proposal.clone());
						}
					}
				}
				Ok((filtered_unsigned_proposals, all_proposals))
			},
			Err(e) => {
				dkg_worker
					.logger
					.error(format!("🕸️  PARTY {party_i} | Failed to get unsigned proposals: {e:?}"));
				Err(DKGError::GenericError {
					reason: format!("Failed to get unsigned proposals: {e:?}"),
				})
			},
		}
	}

	#[allow(clippy::too_many_arguments)]
	/// Create a seed s where s is keccak256(pk, fN=at, unsignedProposal)
	/// This seed is used in the random number generator to generate a
	/// deterministic signing set of size t+1
	async fn maybe_create_signing_set(
		&self,
		dkg_worker: &DKGWorker<B, BE, C, GE>,
		dkg_pub_key: &[u8],
		unsigned_proposal_bytes: &[u8],
		ssid: SSID,
		threshold: u16,
		_session_id: SessionId,
		best_authorities: &[(KeygenPartyId, Public)],
		header: &B::Header,
	) -> Option<ProposedSigningSet> {
		let concat_data = dkg_pub_key
			.iter()
			.copied()
			.chain(unsigned_proposal_bytes.to_owned())
			.chain(ssid.encode())
			.collect::<Vec<u8>>();
		let seed = sp_core::keccak_256(&concat_data);
		let locally_proposed_at: u64 = (*header.number()).saturated_into();

		let maybe_set =
			self.generate_signers(&seed, threshold, best_authorities, dkg_worker).await.ok();

		maybe_set.map(|signing_set| ProposedSigningSet {
			locally_proposed_at,
			ssid,
			signing_set: HashSet::from_iter(signing_set),
		})
	}

	/// After keygen, this should be called to generate a random set of signers
	/// NOTE: since the random set is called using a deterministic seed to and RNG,
	/// the resulting set is deterministic
	async fn generate_signers(
		&self,
		seed: &[u8],
		t: u16,
		best_authorities: &[(KeygenPartyId, Public)],
		dkg_worker: &DKGWorker<B, BE, C, GE>,
	) -> Result<Vec<KeygenPartyId>, DKGError> {
		let only_public_keys = best_authorities.iter().map(|(_, p)| p).cloned().collect::<Vec<_>>();
		let mut final_set = dkg_worker.get_unjailed_signers(&only_public_keys).await?;
		// Mutate the final set if we don't have enough unjailed signers
		if final_set.len() <= t as usize {
			let jailed_set = dkg_worker.get_jailed_signers(&only_public_keys).await?;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposedSigningSet {
	pub ssid: SSID,
	pub locally_proposed_at: u64,
	pub signing_set: HashSet<KeygenPartyId>,
}

impl ProposedSigningSet {
	fn into_sorted_vec(self) -> Vec<KeygenPartyId> {
		self.signing_set.into_iter().sorted().collect()
	}
}

impl Hash for ProposedSigningSet {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.clone().into_sorted_vec().hash(state)
	}
}

pub enum SigningResult {
	Success { unsigned_proposal_hash: [u8; 32] },
	Failure { unsigned_proposal_hash: [u8; 32], ssid: SSID },
}
