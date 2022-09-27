use std::{
	collections::{HashSet, VecDeque},
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	},
};

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	crypto::{AuthorityId, Public},
	types::{DKGError, RoundId},
	UnsignedProposal,
};
use futures::prelude::*;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen::LocalKey,
	sign::{CompletedOfflineStage, OfflineStage},
};
use parking_lot::RwLock;
use rand::{seq::IteratorRandom, Rng};
use tokio::sync::broadcast::Sender;

use crate::utils::find_index;

pub struct DKGSigningManager {
	signers: Vec<DKGSigingSet>,
	message_queue: MessageToSignQueue<UnsignedProposal>,
	from_controller: Sender<Command>,
	offline_stages_per_set: Arc<AtomicUsize>,
}

pub struct DKGSigningManagerController {
	to_worker: Sender<Command>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
	SignMessage(UnsignedProposal),
	Stop,
}

pub struct DKGSigningManagerBuilder {
	pub local_key: LocalKey<Secp256k1>,
	pub best_authorities: Vec<Public>,
	pub authority_public_key: Public,
	pub round_id: RoundId,
	pub threshold: u16,
	pub offline_stages_per_set: usize,
	pub initial_signing_sets: Vec<HashSet<u16>>,
}

#[derive(Clone)]
struct DKGSigingSet {
	s: Vec<u16>,
	local_key: LocalKey<Secp256k1>,
	best_authorities: Vec<Public>,
	authority_public_key: Public,
	round_id: RoundId,
	completed_offline_stages: Vec<CompletedOfflineStage>,
}

impl std::fmt::Debug for DKGSigingSet {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DKGSigingSet")
			.field("s", &self.s)
			.field("completed_offline_stages", &"<hidden>")
			.field("best_authorities", &self.best_authorities)
			.field("authority_public_key", &self.authority_public_key)
			.field("round_id", &self.round_id)
			.field("local_key", &"<hidden>")
			.finish()
	}
}

impl DKGSigingSet {
	fn pick_next_completed_offline_stage(&mut self) -> Option<CompletedOfflineStage> {
		self.completed_offline_stages.pop()
	}

	async fn generate_new_completed_offline_stages(&mut self, count: usize) {
		let party_ind =
			find_index::<AuthorityId>(&self.best_authorities, &self.authority_public_key)
				.map(|r| r as u16 + 1);
		let maybe_offline_i = (1..)
			.zip(&self.s)
			.find(|(_, party_idx)| party_ind == Some(**party_idx))
			.map(|(i, _)| i);
		let offline_i = match maybe_offline_i {
			Some(offline_i) => offline_i,
			None => {
				log::warn!("Not in the signing set");
				return
			},
		};
		let offline_stages = (0..count)
			.map(|_| OfflineStage::new(offline_i, self.s.clone(), self.local_key.clone()));

		// TODO: spawn tasks for each offline stage to be completed.
	}
}
impl Eq for DKGSigingSet {}
impl PartialEq for DKGSigingSet {
	fn eq(&self, other: &Self) -> bool {
		let mut a = self.s.clone();
		let mut b = other.s.clone();
		a.sort_unstable();
		b.sort_unstable();
		a == b
	}
}

impl std::hash::Hash for DKGSigingSet {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		let mut s = self.s.clone();
		s.sort_unstable();
		s.hash(state);
	}
}

#[derive(Clone)]
struct MessageToSignQueue<M> {
	inner: Arc<RwLock<VecDeque<M>>>,
}

impl<M> MessageToSignQueue<M> {
	pub fn new() -> Self {
		Self { inner: Arc::new(RwLock::new(VecDeque::with_capacity(256))) }
	}

	pub fn enqueue(&self, msg: M) {
		self.inner.write().push_back(msg);
	}

	pub fn dequeue(&self) -> Option<M> {
		self.inner.write().pop_front()
	}

	pub fn peek(&self) -> Option<M>
	where
		M: Clone,
	{
		self.inner.read().front().cloned()
	}

	pub fn is_empty(&self) -> bool {
		self.inner.read().is_empty()
	}

	pub fn len(&self) -> usize {
		self.inner.read().len()
	}
}

impl DKGSigningManagerBuilder {
	pub fn build(self) -> (DKGSigningManager, DKGSigningManagerController) {
		let (to_worker, _) = tokio::sync::broadcast::channel(256);
		let from_controller = to_worker.clone();
		let message_queue = MessageToSignQueue::new();
		let offline_stages_per_set = Arc::new(AtomicUsize::new(self.offline_stages_per_set));
		let signers = self
			.initial_signing_sets
			.into_iter()
			.map(|s| {
				let mut s = s.into_iter().collect::<Vec<_>>();
				s.sort_unstable();
				let local_key = self.local_key.clone();
				let best_authorities = self.best_authorities.clone();
				let authority_public_key = self.authority_public_key.clone();
				let round_id = self.round_id;
				let completed_offline_stages = Vec::new();
				DKGSigingSet {
					s,
					local_key,
					best_authorities,
					authority_public_key,
					round_id,
					completed_offline_stages,
				}
			})
			// Collect into a HashSet to remove duplicates.
			.collect::<HashSet<_>>()
			.into_iter()
			// then, collect into a Vec to get deterministic order.
			.collect();
		(
			DKGSigningManager { message_queue, signers, from_controller, offline_stages_per_set },
			DKGSigningManagerController { to_worker },
		)
	}
}

#[async_trait::async_trait]
impl super::SigningManager for DKGSigningManagerController {
	type Message = UnsignedProposal;
	async fn sign(&self, msg: Self::Message) -> Result<(), DKGError> {
		self.to_worker.send(Command::SignMessage(msg)).map(|_| ()).map_err(|_| {
			DKGError::CriticalError {
				reason: String::from("Failed to send message to signing manager"),
			}
		})
	}

	fn stop(&self) {
		self.to_worker
			.send(Command::Stop)
			.expect("Failed to send stop command to signing manager");
	}
}

impl DKGSigningManager {
	pub async fn run(self) {
		let mut worker = self;
		let command_channel = worker.from_controller.subscribe();
		let mut command_stream = tokio_stream::wrappers::BroadcastStream::new(command_channel);
		// a background timer, that fires every few milliseconds, that checks if there any messages
		// to be signed and if there are, it tries to sign them. This is mostly due to the fact that
		// we could fail during the signing process, and we want to retry again later, probably with
		// a different signers, so we push the unsigned message back to the queue and hope that we
		// will see it again later.
		let mut timer = tokio::time::interval(core::time::Duration::from_millis(500));
		let background_queue = worker.message_queue.clone();
		let background_controller = worker.from_controller.clone();
		let background_checker = async move {
			loop {
				timer.tick().await;
				if background_queue.is_empty() {
					continue
				}
				let msg = background_queue.dequeue().expect("is_empty() == false");
				// send the message back to the manager to be signed again.
				background_controller
					.send(Command::SignMessage(msg))
					.expect("Failed to send message to signing manager");
			}
		};
		let handle = tokio::spawn(background_checker);
		// before we start processing any commands, we will need to generate offline stages for all
		// of the signers.
		worker.generate_offline_stages().await;
		// then the main worker loop.
		while let Some(Ok(command)) = command_stream.next().await {
			match command {
				Command::SignMessage(msg) =>
					if let Err(e) = worker.sign_message(msg).await {
						log::error!("Failed to sign message: {:?}", e);
					},
				Command::Stop => break,
			}
		}
		// if the above loop breaks, it means that the task was stopped, so we need to stop the
		// background task as well.
		handle.abort();
	}

	async fn generate_offline_stages(&mut self) {
		let count = self.offline_stages_per_set.load(Ordering::Relaxed);
		for signer in self.signers.iter_mut() {
			signer.generate_new_completed_offline_stages(count).await;
		}
	}

	async fn sign_message(&mut self, message: UnsignedProposal) -> Result<(), DKGError> {
		// enqueue message to be signed
		self.message_queue.enqueue(message);
		// and dequeue the next message to be signed and try to sign it.
		if let Some(msg) = self.message_queue.dequeue() {
			let completed_offline_stage = self.pick_next_completed_offline_stage()?;
			// TODO: Start voting on the completed offline stage
		}

		Ok(())
	}

	fn pick_next_completed_offline_stage(&mut self) -> Result<CompletedOfflineStage, DKGError> {
		// FIXME: use a seed to pick a random signer.
		// like the hash of the public key of the current DKG.
		let mut rng = rand::thread_rng();
		// pick a random number of retries between 5 and 15.
		let retries = rng.gen_range(5..15);
		for _ in 0..retries {
			// pick a random signer set.
			let signers = self.signers.iter_mut().choose(&mut rng).expect("signers is not empty");
			// ask the signers to pick an already completed offline stage.
			let completed_offline_stage = signers.pick_next_completed_offline_stage();
			if let Some(completed_offline_stage) = completed_offline_stage {
				return Ok(completed_offline_stage)
			}
		}
		Err(DKGError::CriticalError {
			reason: format!("Failed to pick a completed offline stage after {} retries", retries),
		})
	}
}
