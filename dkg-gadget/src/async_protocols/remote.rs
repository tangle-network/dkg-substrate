// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{async_protocols::CurrentRoundBlame, debug_logger::DebugLogger};
use atomic::Atomic;
use dkg_primitives::types::{DKGError, SessionId, SignedDKGMessage};
use dkg_runtime_primitives::{crypto::Public, KEYGEN_TIMEOUT, SIGN_TIMEOUT};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use std::sync::{atomic::Ordering, Arc};

pub struct AsyncProtocolRemote<C> {
	pub(crate) status: Arc<Atomic<MetaHandlerStatus>>,
	broadcaster: tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>,
	// allows messages to become enqueued before the protocol is started
	init_handle: ReceiveHandle,
	start_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
	pub(crate) start_rx: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
	stop_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>>,
	pub(crate) stop_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
	pub(crate) started_at: C,
	pub(crate) is_primary_remote: bool,
	current_round_blame: tokio::sync::watch::Receiver<CurrentRoundBlame>,
	pub(crate) current_round_blame_tx: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
	pub(crate) session_id: SessionId,
	pub(crate) logger: DebugLogger,
	status_history: Arc<Mutex<Vec<MetaHandlerStatus>>>,
}

type ReceiveHandle =
	Arc<Mutex<Option<tokio::sync::broadcast::Receiver<Arc<SignedDKGMessage<Public>>>>>>;

impl<C: Clone> Clone for AsyncProtocolRemote<C> {
	fn clone(&self) -> Self {
		Self {
			status: self.status.clone(),
			init_handle: self.init_handle.clone(),
			broadcaster: self.broadcaster.clone(),
			start_tx: self.start_tx.clone(),
			start_rx: self.start_rx.clone(),
			stop_tx: self.stop_tx.clone(),
			stop_rx: self.stop_rx.clone(),
			started_at: self.started_at.clone(),
			is_primary_remote: false,
			current_round_blame: self.current_round_blame.clone(),
			current_round_blame_tx: self.current_round_blame_tx.clone(),
			session_id: self.session_id,
			logger: self.logger.clone(),
			status_history: self.status_history.clone(),
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MetaHandlerStatus {
	Beginning,
	Keygen,
	AwaitingProposals,
	OfflineAndVoting,
	Complete,
	Terminated,
}

impl<C: AtLeast32BitUnsigned + Copy + Send> AsyncProtocolRemote<C> {
	/// Create at the beginning of each meta handler instantiation
	pub fn new(at: C, session_id: SessionId, logger: DebugLogger) -> Self {
		let (stop_tx, stop_rx) = tokio::sync::mpsc::unbounded_channel();
		let (broadcaster, init_handle) = tokio::sync::broadcast::channel(4096);
		let (start_tx, start_rx) = tokio::sync::oneshot::channel();

		let (current_round_blame_tx, current_round_blame) =
			tokio::sync::watch::channel(CurrentRoundBlame::empty());

		let status = Arc::new(Atomic::new(MetaHandlerStatus::Beginning));
		let status_history = Arc::new(Mutex::new(vec![MetaHandlerStatus::Beginning]));

		// let status_debug = status.clone();
		// let status_history_debug = status_history.clone();
		// let logger_debug = logger.clone();

		// The purpose of this task is to log the status of the meta handler
		// in the case that it is stalled/not-progressing. This is useful for debugging.
		// tokio::task::spawn(async move {
		// 	loop {
		// 		tokio::time::sleep(std::time::Duration::from_secs(2)).await;
		// 		let status = status_debug.load(Ordering::Relaxed);
		// 		if [MetaHandlerStatus::Terminated, MetaHandlerStatus::Complete].contains(&status) {
		// 			break
		// 		}
		// 		let status_history = status_history_debug.lock();

		// 		if status == MetaHandlerStatus::Beginning && status_history.len() == 1 {
		// 			continue
		// 		}

		// 		logger_debug.debug(format!(
		// 			"AsyncProtocolRemote status: {status:?} ||||| history: {status_history:?} |||||
		// session_id: {session_id:?}", 		));
		// 	}
		// });

		Self {
			status,
			status_history,
			broadcaster,
			init_handle: Arc::new(Mutex::new(Some(init_handle))),
			started_at: at,
			start_tx: Arc::new(Mutex::new(Some(start_tx))),
			start_rx: Arc::new(Mutex::new(Some(start_rx))),
			stop_tx: Arc::new(Mutex::new(Some(stop_tx))),
			stop_rx: Arc::new(Mutex::new(Some(stop_rx))),
			current_round_blame,
			logger,
			current_round_blame_tx: Arc::new(current_round_blame_tx),
			is_primary_remote: false,
			session_id,
		}
	}

	pub fn set_as_primary(&mut self) {
		self.is_primary_remote = true;
	}

	pub fn keygen_has_stalled(&self, now: C) -> bool {
		self.keygen_is_not_complete() && (now >= self.started_at + KEYGEN_TIMEOUT.into())
	}

	pub fn signing_has_stalled(&self, now: C) -> bool {
		self.signing_is_not_complete() && (now >= self.started_at + SIGN_TIMEOUT.into())
	}

	pub fn keygen_is_not_complete(&self) -> bool {
		self.get_status() != MetaHandlerStatus::Complete ||
			self.get_status() == MetaHandlerStatus::Terminated
	}

	pub fn signing_is_not_complete(&self) -> bool {
		self.get_status() != MetaHandlerStatus::Complete ||
			self.get_status() == MetaHandlerStatus::Terminated
	}
}

impl<C> AsyncProtocolRemote<C> {
	pub fn get_status(&self) -> MetaHandlerStatus {
		self.status.load(Ordering::SeqCst)
	}

	#[track_caller]
	pub fn set_status(&self, status: MetaHandlerStatus) {
		// Validate that the status is being set in the correct order
		#[allow(clippy::match_like_matches_macro)]
		let should_update = match (self.get_status(), status) {
			(MetaHandlerStatus::Beginning, MetaHandlerStatus::Keygen) => true,
			(MetaHandlerStatus::Beginning, MetaHandlerStatus::OfflineAndVoting) => true,
			(MetaHandlerStatus::Beginning, MetaHandlerStatus::Terminated) => true,
			(MetaHandlerStatus::Keygen, MetaHandlerStatus::Complete) => true,
			(MetaHandlerStatus::Keygen, MetaHandlerStatus::Terminated) => true,
			(MetaHandlerStatus::OfflineAndVoting, MetaHandlerStatus::Complete) => true,
			(MetaHandlerStatus::OfflineAndVoting, MetaHandlerStatus::Terminated) => true,
			_ => false,
		};
		if should_update {
			self.status_history.lock().push(status);
			self.status.store(status, Ordering::SeqCst);
		} else {
			// for now, set the state anyways
			self.status_history.lock().push(status);
			self.status.store(status, Ordering::SeqCst);

			self.logger.error(format!(
				"Invalid status update: {:?} -> {:?}",
				self.get_status(),
				status
			));
		}
	}

	pub fn is_active(&self) -> bool {
		let status = self.get_status();
		status != MetaHandlerStatus::Complete && status != MetaHandlerStatus::Terminated
	}

	pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Arc<SignedDKGMessage<Public>>> {
		if let Some(rx) = self.init_handle.lock().take() {
			rx
		} else {
			self.broadcaster.subscribe()
		}
	}

	pub fn deliver_message(
		&self,
		msg: Arc<SignedDKGMessage<Public>>,
	) -> Result<(), tokio::sync::broadcast::error::SendError<Arc<SignedDKGMessage<Public>>>> {
		let status = self.get_status();
		let can_deliver =
			status != MetaHandlerStatus::Complete && status != MetaHandlerStatus::Terminated;
		if self.broadcaster.receiver_count() != 0 && can_deliver {
			self.broadcaster.send(msg).map(|_| ())
		} else {
			// do not forward the message (TODO: Consider enqueuing messages for rounds not yet
			// active other nodes may be active, but this node is still in the process of "waking
			// up"). Thus, by not delivering a message here, we may be preventing this node from
			// joining.
			self.logger.warn(format!("Did not deliver message {:?}", msg.msg.payload));
			Ok(())
		}
	}

	/// Determines if there are any active listeners
	#[allow(dead_code)]
	pub fn is_receiving(&self) -> bool {
		self.broadcaster.receiver_count() != 0
	}

	/// Stops the execution of the meta handler, including all internal asynchronous subroutines
	pub fn start(&self) -> Result<(), DKGError> {
		let tx = self.start_tx.lock().take().ok_or_else(|| DKGError::GenericError {
			reason: "Start has already been called".to_string(),
		})?;
		tx.send(()).map_err(|_| DKGError::GenericError {
			reason: "Unable to send start signal (already shut down?)".to_string(),
		})
	}

	/// Stops the execution of the meta handler, including all internal asynchronous subroutines
	pub fn shutdown<R: AsRef<str>>(&self, reason: R) -> Result<(), DKGError> {
		// check the state if it is active so that we can send a shutdown signal.
		let tx = match self.stop_tx.lock().take() {
			Some(tx) => tx,
			None => {
				self.logger.warn(format!(
					"Unable to shutdown meta handler since it is already {:?}, ignoring...",
					self.get_status()
				));
				return Ok(())
			},
		};
		self.logger.warn(format!("Shutting down meta handler: {}", reason.as_ref()));
		tx.send(()).map_err(|_| DKGError::GenericError {
			reason: "Unable to send shutdown signal (already shut down?)".to_string(),
		})
	}

	pub fn is_keygen_finished(&self) -> bool {
		self.is_completed()
	}

	pub fn is_signing_finished(&self) -> bool {
		self.is_completed()
	}

	pub fn is_completed(&self) -> bool {
		let state = self.get_status();
		matches!(state, MetaHandlerStatus::Complete)
	}

	pub fn is_terminated(&self) -> bool {
		let state = self.get_status();
		matches!(state, MetaHandlerStatus::Terminated)
	}

	pub fn is_done(&self) -> bool {
		self.is_terminated() || self.is_completed()
	}

	pub fn current_round_blame(&self) -> CurrentRoundBlame {
		self.current_round_blame.borrow().clone()
	}
}

impl<C> Drop for AsyncProtocolRemote<C> {
	fn drop(&mut self) {
		if Arc::strong_count(&self.status) == 1 || self.is_primary_remote {
			if self.get_status() != MetaHandlerStatus::Complete {
				self.logger.info(format!(
					"MetaAsyncProtocol is ending: {:?}, History: {:?}",
					self.get_status(),
					self.status_history.lock()
				));
				// if it is not complete, then it must be terminated, since the only other
				// way to exit is to complete.
				if self.get_status() != MetaHandlerStatus::Terminated {
					self.set_status(MetaHandlerStatus::Terminated);
				}
			}

			let _ = self.shutdown("drop code");
		}
	}
}
