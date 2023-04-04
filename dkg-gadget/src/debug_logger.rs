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
		self.log_to_file("trace", &message);
		trace!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn debug<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("debug", &message);
		debug!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn info<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("info", &message);
		info!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn warn<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("warn", &message);
		warn!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	pub fn error<T: std::fmt::Debug>(&self, message: T) {
		self.log_to_file("error", &message);
		error!(target: "dkg_gadget", "[{}]: {message:?}", self.identifier);
	}

	fn log_to_file<T: std::fmt::Debug>(&self, level: &str, message: T) {
		if let Some(file) = &self.to_file_io {
			let message = format!("[{level}]: {message:?}");
			file.send(message).unwrap();
		}
	}
}
