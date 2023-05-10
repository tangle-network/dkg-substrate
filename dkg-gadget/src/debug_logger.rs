#![allow(clippy::unwrap_used)]
use crate::async_protocols::ProtocolType;
use dkg_logging::{debug, error, info, trace, warn};
use parking_lot::RwLock;
use sp_core::Get;
use std::{collections::HashMap, fmt::Debug, io::Write, sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct DebugLogger {
	identifier: Arc<RwLock<String>>,
	to_file_io: tokio::sync::mpsc::UnboundedSender<MessageType>,
	file_handle: Arc<RwLock<Option<std::fs::File>>>,
	events_file_handle_keygen: Arc<RwLock<Option<std::fs::File>>>,
	events_file_handle_signing: Arc<RwLock<Option<std::fs::File>>>,
}

#[derive(Debug, Copy, Clone)]
pub enum AsyncProtocolType {
	Keygen,
	Signing,
}

#[derive(Debug)]
enum MessageType {
	Default(String),
	Event(RoundsEvent),
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
	SentMessage { session: usize, round: usize, sender: u16, receiver: Option<u16> },
	ReceivedMessage { session: usize, round: usize, sender: u16, receiver: Option<u16> },
	ProcessedMessage { session: usize, round: usize, sender: u16, receiver: Option<u16> },
	ProceededToRound { session: usize, round: usize },
	// this probably shouldn't happen, but just in case, we will emit events if this does occur
	PartyIndexChanged { previous: usize, new: usize },
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
		match &self.event {
			RoundsEventType::SentMessage { session, round, receiver, .. } => {
				let receiver = get_legible_name(*receiver);
				writeln!(f, "{me} sent a message to {receiver} for session {session} round {round}")
			},
			RoundsEventType::ReceivedMessage { session, round, sender, receiver } => {
				let msg_type = receiver.map(|_| "direct").unwrap_or("broadcast");
				let sender = get_legible_name(Some(*sender));
				writeln!(f, "{me} received a {msg_type} message from {sender} for session {session} round {round}")
			},
			RoundsEventType::ProcessedMessage { session, round, sender, receiver } => {
				let msg_type = receiver.map(|_| "direct").unwrap_or("broadcast");
				let sender = get_legible_name(Some(*sender));
				writeln!(f, "{me} processed a {msg_type} message from {sender} for session {session} round {round}")
			},
			RoundsEventType::ProceededToRound { session, round } => {
				writeln!(f, "\n~~~~~~~~~~~~~~~~~ {me} Proceeded to round {round} for session {session} ~~~~~~~~~~~~~~~~~")
			},
			RoundsEventType::PartyIndexChanged { previous, new } => {
				writeln!(f, "!!!! Party index changed from {previous} to {new} !!!!")
			},
		}
	}
}

impl DebugLogger {
	pub fn new<T: ToString>(
		identifier: T,
		file: Option<std::path::PathBuf>,
	) -> std::io::Result<Self> {
		// use a channel for sending file I/O requests to a dedicated thread to avoid blocking the
		// DKG workers

		let (file, events_file_keygen, events_file_signing) = Self::get_files(file)?;

		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let file_handle = Arc::new(RwLock::new(file));
		let fh_task = file_handle.clone();

		let events_file_handle = Arc::new(RwLock::new(events_file_keygen));
		let events_fh_task = events_file_handle.clone();

		let events_file_handle_signing = Arc::new(RwLock::new(events_file_signing));
		let events_fh_task_signing = events_file_handle_signing.clone();

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
							AsyncProtocolType::Signing => {
								if let Some(file) = events_fh_task_signing.write().as_mut() {
									writeln!(file, "{event:?}").unwrap();
								}
							},
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
		})
	}

	fn get_files(
		base_output: Option<std::path::PathBuf>,
	) -> std::io::Result<(Option<std::fs::File>, Option<std::fs::File>, Option<std::fs::File>)> {
		if let Some(file_path) = &base_output {
			let file = std::fs::File::create(file_path)?;
			let events_file =
				std::fs::File::create(format!("{}.keygen.events", file_path.display()))?;
			let events_file_signing =
				std::fs::File::create(format!("{}.signing.events", file_path.display()))?;
			Ok((Some(file), Some(events_file), Some(events_file_signing)))
		} else {
			Ok((None, None, None))
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

	pub fn set_output(&self, file: Option<std::path::PathBuf>) -> std::io::Result<()> {
		let (file, event_file, signing_file) = Self::get_files(file)?;
		*self.file_handle.write() = file;
		*self.events_file_handle_keygen.write() = event_file;
		*self.events_file_handle_signing.write() = signing_file;
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
}

impl<T: Get<u32> + Clone + Send + Sync + std::fmt::Debug + 'static> From<&'_ ProtocolType<T>>
	for AsyncProtocolType
{
	fn from(value: &ProtocolType<T>) -> Self {
		if matches!(value, ProtocolType::Keygen { .. }) {
			AsyncProtocolType::Keygen
		} else {
			AsyncProtocolType::Signing
		}
	}
}
