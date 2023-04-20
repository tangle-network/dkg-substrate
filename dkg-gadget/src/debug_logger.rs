use dkg_logging::{debug, error, info, trace, warn};
use std::{io::Write, sync::Arc};

#[derive(Clone, Debug)]
pub struct DebugLogger {
	identifier: Arc<String>,
	to_file_io: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl DebugLogger {
	pub fn new<T: ToString>(identifier: T, file: Option<std::fs::File>) -> Self {
		// use a channel for sending file I/O requests to a dedicated thread to avoid blocking the
		// DKG workers
		if let Some(mut file) = file {
			let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

			tokio::task::spawn(async move {
				while let Some(message) = rx.recv().await {
					writeln!(file, "{message:?}").unwrap();
				}
			});

			Self { identifier: Arc::new(identifier.to_string()), to_file_io: Some(tx) }
		} else {
			Self { identifier: Arc::new(identifier.to_string()), to_file_io: None }
		}
	}

	pub fn trace<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "trace", &message);
		trace!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn debug<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "debug", &message);
		debug!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn info<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "info", &message);
		info!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn warn<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "warn", &message);
		warn!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn error<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget", "error", &message);
		error!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn trace_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "trace", &message);
		trace!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.identifier);
	}

	pub fn debug_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "debug", &message);
		debug!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.identifier);
	}

	pub fn info_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "info", &message);
		info!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.identifier);
	}

	pub fn warn_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "warn", &message);
		warn!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.identifier);
	}

	pub fn error_signing<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::signing", "error", &message);
		error!(target: "dkg_gadget::signing", "[{}]: {message:?}", self.identifier);
	}

	pub fn trace_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "trace", &message);
		trace!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.identifier);
	}

	pub fn debug_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "debug", &message);
		debug!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.identifier);
	}

	pub fn info_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "info", &message);
		info!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.identifier);
	}

	pub fn warn_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "warn", &message);
		warn!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.identifier);
	}

	pub fn error_keygen<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("dkg_gadget::async_protocol::keygen", "error", &message);
		error!(target: "dkg_gadget::keygen", "[{}]: {message:?}", self.identifier);
	}

	fn log_to_file<T: std::fmt::Debug>(&self, target: &str, level: &str, message: T) {
		if let Some(file) = &self.to_file_io {
			let message = format!("[{target}] [{level}]: {message:?}");
			if let Err(err) = file.send(message) {
				error!(target: "dkg_gadget", "failed to send log message to file: {err:?}");
			}
		}
	}
}
