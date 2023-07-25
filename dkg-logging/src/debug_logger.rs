#![allow(clippy::unwrap_used)]
use crate::{debug, error, info, trace, warn};
use lazy_static::lazy_static;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use sp_core::{bytes::to_hex, hashing::sha2_256};
use std::{collections::HashMap, fmt::Debug, io::Write, path::PathBuf, sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct DebugLogger {
	identifier: Arc<RwLock<String>>,
	to_file_io: tokio::sync::mpsc::UnboundedSender<MessageType>,
	file_handle: Arc<RwLock<Option<std::fs::File>>>,
	events_file_handle_keygen: Arc<RwLock<Option<std::fs::File>>>,
	events_file_handle_signing: Arc<RwLock<Option<std::fs::File>>>,
	events_file_handle_voting: Arc<RwLock<Option<std::fs::File>>>,
	checkpoints_enabled: bool,
	paths: Arc<Mutex<Vec<PathBuf>>>,
}

struct Checkpoint {
	checkpoint: String,
	message_hash: String,
}

lazy_static! {
	static ref CHECKPOINTS: RwLock<HashMap<String, Checkpoint>> = RwLock::new(HashMap::new());
}

#[derive(Debug, Copy, Clone)]
pub enum AsyncProtocolType {
	Keygen,
	Signing { hash: [u8; 32] },
	Voting { hash: [u8; 32] },
}

#[derive(Debug)]
enum MessageType {
	Default(String),
	Event(RoundsEvent),
	Clear,
}

lazy_static::lazy_static! {
	static ref INIT_TIME: Instant = Instant::now();
	static ref NAMES_MAP: RwLock<HashMap<String, &'static str>> = RwLock::new(HashMap::new());
	static ref PARTY_I_MAP: RwLock<HashMap<usize, String>> = RwLock::new(HashMap::new());
}

// names for mapping the uuids to a human-readable name
const NAMES: &[&str] = &[
	"Alice", "Bob", "Charlie", "Dave", "Eve", "Faythe", "Grace", "Heidi", "Ivan", "Judy",
	"Mallory", "Niaj", "Olivia", "Peggy", "Rupert", "Sybil", "Trent", "Walter", "Wendy", "Zach",
];

pub struct RoundsEvent {
	name: String,
	event: RoundsEventType,
	proto: AsyncProtocolType,
}
pub enum RoundsEventType {
	SentMessage {
		session: usize,
		round: usize,
		sender: u16,
		receiver: Option<u16>,
		msg_hash: String,
	},
	ReceivedMessage {
		session: usize,
		round: usize,
		sender: u16,
		receiver: Option<u16>,
		msg_hash: String,
	},
	ProcessedMessage {
		session: usize,
		round: usize,
		sender: u16,
		receiver: Option<u16>,
		msg_hash: String,
	},
	ProceededToRound {
		session: u64,
		round: usize,
	},
	// this probably shouldn't happen, but just in case, we will emit events if this does occur
	PartyIndexChanged {
		previous: usize,
		new: usize,
	},
}

impl RoundsEventType {
	fn sender(&self) -> Option<usize> {
		match self {
			RoundsEventType::SentMessage { sender, .. } => Some(*sender as usize),
			RoundsEventType::ReceivedMessage { sender, .. } => Some(*sender as usize),
			RoundsEventType::ProcessedMessage { sender, .. } => Some(*sender as usize),
			_ => None,
		}
	}
}

impl AsyncProtocolType {
	fn hash(&self) -> Option<&[u8; 32]> {
		match self {
			AsyncProtocolType::Keygen => None,
			AsyncProtocolType::Signing { hash } => Some(hash),
			AsyncProtocolType::Voting { hash } => Some(hash),
		}
	}
}

fn get_legible_name(idx: Option<u16>) -> String {
	if let Some(party_i) = idx {
		let party_i = party_i as usize;
		if let Some(uuid) = PARTY_I_MAP.read().get(&party_i).cloned() {
			if let Some(name) = NAMES_MAP.read().get(&uuid).cloned() {
				name.to_string()
			} else {
				party_i.to_string()
			}
		} else {
			party_i.to_string()
		}
	} else {
		"everyone".to_string()
	}
}

impl Debug for RoundsEvent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let me = &self.name;
		let hash_opt = self.proto.hash().map(hex::encode);
		let hash_str =
			hash_opt.map(|hash| format!(" unsigned proposal {hash}")).unwrap_or_default();
		match &self.event {
			RoundsEventType::SentMessage { session, round, receiver, msg_hash, .. } => {
				let receiver = get_legible_name(*receiver);
				writeln!(f, "{me} sent a message to {receiver} for session {session} round {round}{hash_str} | {msg_hash}")
			},
			RoundsEventType::ReceivedMessage { session, round, sender, receiver, msg_hash } => {
				let msg_type = receiver.map(|_| "direct").unwrap_or("broadcast");
				let sender = get_legible_name(Some(*sender));
				writeln!(f, "{me} received a {msg_type} message from {sender} for session {session} round {round}{hash_str}| {msg_hash}")
			},
			RoundsEventType::ProcessedMessage { session, round, sender, receiver, msg_hash } => {
				let msg_type = receiver.map(|_| "direct").unwrap_or("broadcast");
				let sender = get_legible_name(Some(*sender));
				writeln!(f, "{me} processed a {msg_type} message from {sender} for session {session} round {round}{hash_str}| {msg_hash}")
			},
			RoundsEventType::ProceededToRound { session, round } => {
				writeln!(f, "\n~~~~~~~~~~~~~~~~~ {me} Proceeded to round {round} for session {session} {hash_str} ~~~~~~~~~~~~~~~~~")
			},
			RoundsEventType::PartyIndexChanged { previous, new } => {
				writeln!(f, "!!!! Party index changed from {previous} to {new} !!!!")
			},
		}
	}
}

type EventFiles =
	(Option<std::fs::File>, Option<std::fs::File>, Option<std::fs::File>, Option<std::fs::File>);

impl DebugLogger {
	pub fn new<T: ToString>(identifier: T, file: Option<PathBuf>) -> std::io::Result<Self> {
		// use a channel for sending file I/O requests to a dedicated thread to avoid blocking the
		// DKG workers

		let checkpoints_enabled = std::env::var("CHECKPOINTS").unwrap_or_default() == "enabled";
		if checkpoints_enabled {
			static HAS_CHECKPOINT_TRACKER_RUN: std::sync::atomic::AtomicBool =
				std::sync::atomic::AtomicBool::new(false);
			if !HAS_CHECKPOINT_TRACKER_RUN.swap(true, std::sync::atomic::Ordering::Relaxed) {
				// spawn a task to periodically print out the last checkpoint for each message
				println!("Running checkpoint tracker");
				tokio::task::spawn(async move {
					loop {
						tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
						let lock = CHECKPOINTS.read();
						for checkpoint in lock.values() {
							warn!(target: "dkg", "Checkpoint for {} last at {}", checkpoint.message_hash, checkpoint.checkpoint);
						}
					}
				});
			}
		}

		let paths = Arc::new(Mutex::new(Vec::new()));
		let paths_task = paths.clone();

		let (file, events_file_keygen, events_file_signing, events_file_voting) =
			Self::get_files(&paths, file)?;

		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let file_handle = Arc::new(RwLock::new(file));
		let fh_task = file_handle.clone();

		let events_file_handle = Arc::new(RwLock::new(events_file_keygen));
		let events_fh_task = events_file_handle.clone();

		let events_file_handle_signing = Arc::new(RwLock::new(events_file_signing));
		let events_fh_task_signing = events_file_handle_signing.clone();

		let events_file_handle_voting = Arc::new(RwLock::new(events_file_voting));
		let events_fh_task_voting = events_file_handle_voting.clone();

		if tokio::runtime::Handle::try_current().is_ok() {
			tokio::task::spawn(async move {
				while let Some(message) = rx.recv().await {
					match message {
						MessageType::Default(message) =>
							if let Some(file) = fh_task.write().as_mut() {
								writeln!(file, "{message}").unwrap();
							},
						MessageType::Event(event) => match event.proto {
							AsyncProtocolType::Keygen => {
								if let Some(file) = events_fh_task.write().as_mut() {
									writeln!(file, "{event:?}").unwrap();
								}
							},
							AsyncProtocolType::Signing { .. } => {
								if let Some(file) = events_fh_task_signing.write().as_mut() {
									writeln!(file, "{event:?}").unwrap();
								}
							},
							AsyncProtocolType::Voting { .. } => {
								if let Some(file) = events_fh_task_voting.write().as_mut() {
									writeln!(file, "{event:?}").unwrap();
								}
							},
						},
						MessageType::Clear => {
							let lock = paths_task.lock();
							for path in &*lock {
								if let Err(err) = std::fs::remove_file(path) {
									error!(target: "dkg", "Failed to remove file: {err}");
								}
							}
						},
					}
				}
			});
		}

		Ok(Self {
			identifier: Arc::new(identifier.to_string().into()),
			to_file_io: tx,
			file_handle,
			events_file_handle_keygen: events_file_handle,
			events_file_handle_signing,
			events_file_handle_voting,
			checkpoints_enabled,
			paths,
		})
	}

	fn get_files(
		paths: &Arc<Mutex<Vec<PathBuf>>>,
		base_output: Option<PathBuf>,
	) -> std::io::Result<EventFiles> {
		if let Some(file_path) = base_output {
			let file = std::fs::File::create(&file_path)?;
			let keygen_path = PathBuf::from(format!("{}.keygen.log", file_path.display()));
			let signing_path = PathBuf::from(format!("{}.signing.log", file_path.display()));
			let voting_path = PathBuf::from(format!("{}.voting.log", file_path.display()));
			let events_file = std::fs::File::create(&keygen_path)?;
			let events_file_signing = std::fs::File::create(&signing_path)?;
			let events_file_voting = std::fs::File::create(&voting_path)?;
			*paths.lock() = vec![file_path, keygen_path, signing_path, voting_path];
			Ok((Some(file), Some(events_file), Some(events_file_signing), Some(events_file_voting)))
		} else {
			Ok((None, None, None, None))
		}
	}

	pub fn set_id<T: ToString>(&self, id: T) {
		let id = id.to_string();
		let mut names_map = NAMES_MAP.write();
		let len = names_map.len();
		assert!(len < NAMES.len());
		names_map.insert(id.clone(), NAMES[len]);
		*self.identifier.write() = id;
	}

	pub fn set_output(&self, file: Option<PathBuf>) -> std::io::Result<()> {
		let (file, event_file, signing_file, voting_file) = Self::get_files(&self.paths, file)?;
		*self.file_handle.write() = file;
		*self.events_file_handle_keygen.write() = event_file;
		*self.events_file_handle_signing.write() = signing_file;
		*self.events_file_handle_voting.write() = voting_file;
		Ok(())
	}

	fn get_identifier(&self) -> String {
		self.identifier.read().to_string()
	}

	pub fn trace<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget", "trace", &message);
		trace!(target: "dkg_gadget", "[{}]: {message}", self.get_identifier());
	}

	pub fn debug<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget", "debug", &message);
		debug!(target: "dkg_gadget", "[{}]: {message}", self.get_identifier());
	}

	pub fn info<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget", "info", &message);
		info!(target: "dkg_gadget", "[{}]: {message}", self.get_identifier());
	}

	pub fn warn<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget", "warn", &message);
		warn!(target: "dkg_gadget", "[{}]: {message}", self.get_identifier());
	}

	pub fn error<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget", "error", &message);
		error!(target: "dkg_gadget", "[{}]: {message}", self.get_identifier());
	}

	pub fn trace_signing<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "trace", &message);
		trace!(target: "dkg_gadget::signing", "[{}]: {message}", self.get_identifier());
	}

	pub fn debug_signing<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "debug", &message);
		debug!(target: "dkg_gadget::signing", "[{}]: {message}", self.get_identifier());
	}

	pub fn info_signing<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "info", &message);
		info!(target: "dkg_gadget::signing", "[{}]: {message}", self.get_identifier());
	}

	pub fn warn_signing<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "warn", &message);
		warn!(target: "dkg_gadget::signing", "[{}]: {message}", self.get_identifier());
	}

	pub fn error_signing<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "error", &message);
		error!(target: "dkg_gadget::signing", "[{}]: {message}", self.get_identifier());
	}

	pub fn trace_keygen<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "trace", &message);
		trace!(target: "dkg_gadget::keygen", "[{}]: {message}", self.get_identifier());
	}

	pub fn debug_keygen<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "debug", &message);
		debug!(target: "dkg_gadget::keygen", "[{}]: {message}", self.get_identifier());
	}

	pub fn info_keygen<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "info", &message);
		info!(target: "dkg_gadget::keygen", "[{}]: {message}", self.get_identifier());
	}

	pub fn warn_keygen<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "warn", &message);
		warn!(target: "dkg_gadget::keygen", "[{}]: {message}", self.get_identifier());
	}

	pub fn error_keygen<T: std::fmt::Display>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "error", &message);
		error!(target: "dkg_gadget::keygen", "[{}]: {message}", self.get_identifier());
	}

	fn log_to_file<T: std::fmt::Display>(&self, target: &str, level: &str, message: T) {
		let time = INIT_TIME.elapsed();
		let message = format!("[{target}] [{level}] [{time:?}] : {message}");
		if let Err(err) = self.to_file_io.send(MessageType::Default(message)) {
			error!(target: "dkg_gadget", "failed to send log message to file: {err:?}");
		}
	}

	pub fn round_event<T: Into<AsyncProtocolType>>(&self, proto: T, event: RoundsEventType) {
		let id = self.identifier.read().clone();
		let proto = proto.into();
		if let Some(sender) = event.sender() {
			if matches!(event, RoundsEventType::SentMessage { .. }) {
				let prev_val = PARTY_I_MAP.write().insert(sender, id.clone());
				if let Some(prev_val) = prev_val {
					if prev_val != id {
						// This means our ID changed. This shouldn't happen in the harness tests
					}
				}
			}
		}

		let name = if let Some(val) = NAMES_MAP.read().get(&id) { val.to_string() } else { id };
		let event = RoundsEvent { name, event, proto };
		if let Err(err) = self.to_file_io.send(MessageType::Event(event)) {
			error!(target: "dkg_gadget", "failed to send event message to file: {err:?}");
		}
	}

	pub fn checkpoint_message<T: Serialize>(&self, msg: T, checkpoint: impl Into<String>) {
		if self.checkpoints_enabled {
			let hash = message_to_string_hash(&msg);
			CHECKPOINTS.write().insert(
				hash.clone(),
				Checkpoint { checkpoint: checkpoint.into(), message_hash: hash },
			);
		}
	}

	pub fn checkpoint_message_raw(&self, payload: &[u8], checkpoint: impl Into<String>) {
		if self.checkpoints_enabled {
			let hash = raw_message_to_hash(payload);
			CHECKPOINTS.write().insert(
				hash.clone(),
				Checkpoint { checkpoint: checkpoint.into(), message_hash: hash },
			);
		}
	}

	pub fn clear_checkpoints(&self) {
		if self.checkpoints_enabled {
			CHECKPOINTS.write().clear();
		}
	}

	pub fn clear_checkpoint_for_message<T: Serialize>(&self, msg: T) {
		if self.checkpoints_enabled {
			let hash = message_to_string_hash(&msg);
			CHECKPOINTS.write().remove(&hash);
		}
	}

	pub fn clear_checkpoint_for_message_raw(&self, payload: &[u8]) {
		if self.checkpoints_enabled {
			let hash = raw_message_to_hash(payload);
			CHECKPOINTS.write().remove(&hash);
		}
	}

	/// Completely deletes all local logs associated with this program
	pub fn clear_local_logs(&self) {
		if let Err(err) = self.to_file_io.send(MessageType::Clear) {
			error!(target: "dkg_gadget", "failed to send event message to file: {err:?}");
		}
	}
}

pub fn message_to_string_hash<T: Serialize>(msg: T) -> String {
	let message = serde_json::to_vec(&msg).expect("message_to_string_hash");
	let message = sha2_256(&message);
	to_hex(&message, false)
}

pub fn raw_message_to_hash(payload: &[u8]) -> String {
	let message = sha2_256(payload);
	to_hex(&message, false)
}
