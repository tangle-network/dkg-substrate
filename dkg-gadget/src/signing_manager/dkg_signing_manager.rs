use std::{
	collections::{HashMap, HashSet, VecDeque},
	sync::Arc,
};

use curv::elliptic::curves::Secp256k1;
use dkg_primitives::{
	crypto::{AuthorityId, Public},
	types::{DKGError, DKGMsgPayload, DKGMsgStatus, RoundId, SignedDKGMessage, Uid},
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

use crate::{
	async_protocols::{
		blockchain_interface::BlockchainInterface,
		new_inner,
		remote::{AsyncProtocolRemote, MetaHandlerStatus},
		AsyncProtocolParameters, ProtocolType,
	},
	utils::find_index,
};

pub struct DKGSigningManager<BI> {
	signers: Vec<DKGSigingSet<BI>>,
	message_queue: MessageToSignQueue<UnsignedProposal>,
	from_controller: Sender<Command>,
}

pub struct DKGSigningManagerController {
	to_worker: Sender<Command>,
}

#[derive(Clone, Debug)]
enum Command {
	SignMessage(UnsignedProposal),
	DeliverDKGMessage(SignedDKGMessage<Public>),
	Stop,
}

pub struct DKGSigningManagerBuilder {
	pub local_key: LocalKey<Secp256k1>,
	pub best_authorities: Vec<Public>,
	pub authority_public_key: Public,
	pub round_id: RoundId,
	pub threshold: u16,
	pub initial_signing_sets: Vec<HashSet<u16>>,
}

struct DKGSigingSet<BI> {
	s: Vec<u16>,
	local_key: LocalKey<Secp256k1>,
	best_authorities: Vec<Public>,
	authority_public_key: Public,
	round_id: RoundId,
	completed_offline_stages: Vec<CompletedOfflineStage>,
	async_proto_params: Vec<AsyncProtocolParameters<BI>>,
	async_remotes: HashMap<Uid, AsyncProtocolRemote>,
}

impl<BI> std::fmt::Debug for DKGSigingSet<BI> {
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

impl<BI> DKGSigingSet<BI> {
	fn pick_next_completed_offline_stage(&mut self) -> Option<CompletedOfflineStage> {
		self.completed_offline_stages.pop()
	}
}

impl<BI> DKGSigingSet<BI>
where
	BI: BlockchainInterface + 'static,
{
	async fn generate_new_completed_offline_stages(&mut self) {
		// sort the current singing set, if not already sorted.
		self.s.sort();
		let count = self.async_proto_params.len();
		// no more offline stages to generate
		if count == 0 {
			return
		}
		let async_proto_params = std::mem::take(&mut self.async_proto_params);
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
			.map(|i| {
				(
					// generate the Unique id for the offline stage
					// by using the counter, and the current signing set and hash them together.
					//
					// the reason we also use the signing set too here is to make sure that
					// the id is unique across all other signing sets.
					Uid::from_hash_of(&(i as u64, &self.s)),
					OfflineStage::new(offline_i, self.s.clone(), self.local_key.clone()).unwrap(),
				)
			})
			.zip(async_proto_params.into_iter());
		let completed_offline_stage = Arc::new(RwLock::new(Vec::new()));

		let mut tasks = Vec::new();
		for ((uid, offline_stage), params) in offline_stages {
			let channel_type = ProtocolType::Offline {
				uid,
				i: offline_i,
				s_l: self.s.clone(),
				local_key: Arc::new(self.local_key.clone()),
			};
			let remote = params.handle.clone();
			let mut stop_rx = remote.stop_rx.lock().take().expect("stop_rx should be set");
			let start_rx = remote.start_rx.lock().take().expect("start_rx should be set");
			// store the remote for the later use.
			self.async_remotes.insert(uid, remote);
			let completed_offline_stage = completed_offline_stage.clone();
			let protocol = async move {
				let inner_handle = params.handle.clone();
				start_rx.await;
				inner_handle.set_status(MetaHandlerStatus::Offline);
				let inner_task = new_inner(
					completed_offline_stage,
					offline_stage,
					params,
					channel_type,
					DKGMsgStatus::ACTIVE,
				)
				.expect("Failed to create new offline protocol")
				.then(|res| async move {
					inner_handle.set_status(MetaHandlerStatus::Complete);
					// print the res value.
					log::info!(target: "dkg", "🕸️  Signing protocol concluded with {:?}", res);
					res
				});
			};
			let task = Box::pin(async move {
				tokio::select! {
					res0 = protocol => Ok(res0),
					res1 = stop_rx.recv() => {
						log::info!(target: "dkg", "Stopper has been called {:?}", res1);
						Err(DKGError::GenericError {
							reason: format!("Stopper has been called {:?}", res1)
						})
					}
				}
			});
			let handle = tokio::spawn(task);
			tasks.push(handle);
		}
		// join all tasks and wait for them to finish
		let _res = futures::future::join_all(tasks).await;
		let completed_offline_stages =
			completed_offline_stage.read().iter().cloned().collect::<Vec<_>>();
		self.completed_offline_stages = completed_offline_stages;
	}

	// deliver_message method is used to deliver messages to the offline protocol.
	fn deliver_message(&mut self, message: SignedDKGMessage<Public>) -> Result<(), DKGError> {
		let uid = if let DKGMsgPayload::Offline(ref payload) = message.msg.payload {
			payload.uid
		} else {
			return Err(DKGError::GenericError {
				reason: "Invalid message type for offline protocol".to_string(),
			})
		};
		if let Some(remote) = self.async_remotes.get(&uid) {
			let msg = Arc::new(message);
			remote.deliver_message(msg).map_err(|e| {
				log::warn!("Failed to deliver message to the offline protocol: {:?}", e);
				DKGError::GenericError {
					reason: "Failed to deliver message to the offline protocol".to_string(),
				}
			})
		} else {
			Err(DKGError::GenericError {
				reason: "Failed to deliver message to the offline protocol".to_string(),
			})
		}
	}
}

impl<BI> Eq for DKGSigingSet<BI> {}
impl<BI> PartialEq for DKGSigingSet<BI> {
	fn eq(&self, other: &Self) -> bool {
		let mut a = self.s.clone();
		let mut b = other.s.clone();
		a.sort_unstable();
		b.sort_unstable();
		a == b
	}
}

impl<BI> PartialOrd for DKGSigingSet<BI> {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		let mut a = self.s.clone();
		let mut b = other.s.clone();
		a.sort_unstable();
		b.sort_unstable();
		a.partial_cmp(&b)
	}
}

impl<BI> Ord for DKGSigingSet<BI> {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		let mut a = self.s.clone();
		let mut b = other.s.clone();
		a.sort_unstable();
		b.sort_unstable();
		a.cmp(&b)
	}
}

impl<BI> std::hash::Hash for DKGSigingSet<BI> {
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
	pub fn build<BI>(
		self,
		async_proto_params: Vec<AsyncProtocolParameters<BI>>,
	) -> (DKGSigningManager<BI>, DKGSigningManagerController)
	where
		BI: BlockchainInterface,
	{
		let (to_worker, _) = tokio::sync::broadcast::channel(256);
		let from_controller = to_worker.clone();
		let message_queue = MessageToSignQueue::new();
		let signers_len = self.initial_signing_sets.len();
		let params_per_signer = async_proto_params.len() / signers_len;
		let params_chunks = async_proto_params.chunks(params_per_signer).into_iter();
		let mut signers: Vec<_> = self
			.initial_signing_sets
			.into_iter()
			.zip(params_chunks)
			.map(|(s, params)| {
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
					async_proto_params: params.to_vec(),
					async_remotes: HashMap::new(),
				}
			})
			// Collect into a HashSet to remove duplicates.
			.collect::<HashSet<_>>()
			.into_iter()
			// then, collect into a Vec to get deterministic order.
			.collect();
		// Sort the signers by the signing set.
		signers.sort();
		(
			DKGSigningManager { message_queue, signers, from_controller },
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

	fn deliver_dkg_message(&self, msg: SignedDKGMessage<Public>) -> Result<(), DKGError> {
		self.to_worker.send(Command::DeliverDKGMessage(msg)).map(|_| ()).map_err(|_| {
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

impl<BI> DKGSigningManager<BI>
where
	BI: BlockchainInterface + 'static,
{
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
				Command::DeliverDKGMessage(msg) =>
					if let Err(e) = worker.deliver_message(msg) {
						log::error!("Failed to deliver DKG message: {:?}", e);
					},
				Command::Stop => break,
			}
		}
		// if the above loop breaks, it means that the task was stopped, so we need to stop the
		// background task as well.
		handle.abort();
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

	fn deliver_message(&mut self, message: SignedDKGMessage<Public>) -> Result<(), DKGError> {
		// ask each signing set to try to deliver this message, as it might be for them.
		for signing_set in self.signers.iter_mut() {
			let delivered = signing_set.deliver_message(message.clone()).is_ok();
			if delivered {
				// if the message was delivered, we can stop here.
				return Ok(())
			}
		}
		// if we got here, it means that the message was not delivered to any of the signing sets.
		Err(DKGError::CriticalError {
			reason: String::from("Failed to deliver message to any signing set"),
		})
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

impl<BI> DKGSigningManager<BI>
where
	BI: BlockchainInterface + 'static,
{
	async fn generate_offline_stages(&mut self) {
		for signer in self.signers.iter_mut() {
			signer.generate_new_completed_offline_stages().await;
		}
	}
}
