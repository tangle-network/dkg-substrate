#![allow(clippy::unwrap_used)]
use dkg_logging::{debug, error, info, trace, warn};
use parking_lot::RwLock;
use std::{io::Write, sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct DebugLogger {
	identifier: Arc<RwLock<String>>,
	to_file_io: tokio::sync::mpsc::UnboundedSender<String>,
	file_handle: Arc<RwLock<Option<std::fs::File>>>,
}

lazy_static::lazy_static! {
	static ref INIT_TIME: Instant = Instant::now();
}

impl DebugLogger {
	pub fn new<T: ToString>(identifier: T, file: Option<std::fs::File>) -> Self {
		// use a channel for sending file I/O requests to a dedicated thread to avoid blocking the
		// DKG workers

		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let file_handle = Arc::new(RwLock::new(file));
		let fh_task = file_handle.clone();

		tokio::task::spawn(async move {
			while let Some(message) = rx.recv().await {
				if let Some(file) = fh_task.write().as_mut() {
					writeln!(file, "{message:?}").unwrap();
				}
			}
		});

		Self {
			identifier: Arc::new(identifier.to_string().into()),
			to_file_io: tx.into(),
			file_handle,
		}
	}

	pub fn set_id<T: ToString>(&self, id: T) {
		*self.identifier.write() = id.to_string();
	}

	pub fn set_output(&self, file: std::fs::File) {
		self.file_handle.write().replace(file);
	}

	fn get_identifier(&self) -> String {
		self.identifier.read().to_string()
	}

	pub fn trace<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "trace", &message);
		trace!(target: "dkg_gadget", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn debug<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "debug", &message);
		debug!(target: "dkg_gadget", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn info<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "info", &message);
		info!(target: "dkg_gadget", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn warn<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "warn", &message);
		warn!(target: "dkg_gadget", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn error<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "error", &message);
		error!(target: "dkg_gadget", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn trace_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "trace", &message);
		trace!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn debug_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "debug", &message);
		debug!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn info_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "info", &message);
		info!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn warn_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "warn", &message);
		warn!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn error_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "error", &message);
		error!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn trace_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "trace", &message);
		trace!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn debug_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "debug", &message);
		debug!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn info_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "info", &message);
		info!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn warn_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "warn", &message);
		warn!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.get_identifier());
	}

	pub fn error_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "error", &message);
		error!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.get_identifier());
	}

	fn log_to_file<T: std::fmt::Debug>(&self, target: &str, level: &str, message: T) {
		let time = INIT_TIME.elapsed();
		let message = format!("[{time:?}] [{target}] [{level}]: {message:?}");
		if let Err(err) = self.to_file_io.send(message) {
			error!(target: "dkg_gadget", "failed to send log message to file: {err:?}");
		}
	}
}
