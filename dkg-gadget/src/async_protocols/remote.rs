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

use crate::async_protocols::CurrentRoundBlame;
use atomic::Atomic;
use dkg_primitives::types::{DKGError, RoundId, SignedDKGMessage};
use dkg_runtime_primitives::{crypto::Public, KEYGEN_TIMEOUT};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use std::sync::{atomic::Ordering, Arc};

pub struct AsyncProtocolRemote<C> {
	pub(crate) status: Arc<Atomic<MetaHandlerStatus>>,
	pub(crate) broadcaster: tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>,
	start_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
	pub(crate) start_rx: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
	stop_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>>,
	pub(crate) stop_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
	pub(crate) started_at: C,
	pub(crate) is_primary_remote: bool,
	current_round_blame: tokio::sync::watch::Receiver<CurrentRoundBlame>,
	pub(crate) current_round_blame_tx: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
	pub(crate) round_id: RoundId,
	status_history: Arc<Mutex<Vec<MetaHandlerStatus>>>,
}

impl<C: Clone> Clone for AsyncProtocolRemote<C> {
	fn clone(&self) -> Self {
		Self {
			status: self.status.clone(),
			broadcaster: self.broadcaster.clone(),
			start_tx: self.start_tx.clone(),
			start_rx: self.start_rx.clone(),
			stop_tx: self.stop_tx.clone(),
			stop_rx: self.stop_rx.clone(),
			started_at: self.started_at.clone(),
			is_primary_remote: false,
			current_round_blame: self.current_round_blame.clone(),
			current_round_blame_tx: self.current_round_blame_tx.clone(),
			round_id: self.round_id,
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

impl<C: AtLeast32BitUnsigned + Copy> AsyncProtocolRemote<C> {
	/// Create at the beginning of each meta handler instantiation
	pub fn new(at: C, round_id: RoundId) -> Self {
		let (stop_tx, stop_rx) = tokio::sync::mpsc::unbounded_channel();
		let (broadcaster, _) = tokio::sync::broadcast::channel(4096);
		let (start_tx, start_rx) = tokio::sync::oneshot::channel();

		let (current_round_blame_tx, current_round_blame) =
			tokio::sync::watch::channel(CurrentRoundBlame::empty());

		Self {
			status: Arc::new(Atomic::new(MetaHandlerStatus::Beginning)),
			status_history: Arc::new(Mutex::new(vec![MetaHandlerStatus::Beginning])),
			broadcaster,
			started_at: at,
			start_tx: Arc::new(Mutex::new(Some(start_tx))),
			start_rx: Arc::new(Mutex::new(Some(start_rx))),
			stop_tx: Arc::new(Mutex::new(Some(stop_tx))),
			stop_rx: Arc::new(Mutex::new(Some(stop_rx))),
			current_round_blame,
			current_round_blame_tx: Arc::new(current_round_blame_tx),
			is_primary_remote: false,
			round_id,
		}
	}

	pub fn keygen_has_stalled(&self, now: C) -> bool {
		self.keygen_is_not_complete() && (now - self.started_at > KEYGEN_TIMEOUT.into())
	}

	pub fn keygen_is_not_complete(&self) -> bool {
		self.get_status() != MetaHandlerStatus::Complete ||
			self.get_status() == MetaHandlerStatus::Terminated
	}
}

impl<C> AsyncProtocolRemote<C> {
	pub fn get_status(&self) -> MetaHandlerStatus {
		self.status.load(Ordering::SeqCst)
	}

	pub fn set_status(&self, status: MetaHandlerStatus) {
		self.status_history.lock().push(status);
		self.status.store(status, Ordering::SeqCst)
	}

	pub fn is_active(&self) -> bool {
		let status = self.get_status();
		status != MetaHandlerStatus::Beginning &&
			status != MetaHandlerStatus::Complete &&
			status != MetaHandlerStatus::Terminated
	}

	pub fn deliver_message(
		&self,
		msg: Arc<SignedDKGMessage<Public>>,
	) -> Result<(), tokio::sync::broadcast::error::SendError<Arc<SignedDKGMessage<Public>>>> {
		if self.broadcaster.receiver_count() != 0 {
			self.broadcaster.send(msg).map(|_| ())
		} else {
			// do not forward the message
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
		let tx = self.stop_tx.lock().take().ok_or_else(|| DKGError::GenericError {
			reason: "Shutdown has already been called".to_string(),
		})?;
		log::info!("Shutting down meta handler: {}", reason.as_ref());
		tx.send(()).map_err(|_| DKGError::GenericError {
			reason: "Unable to send shutdown signal (already shut down?)".to_string(),
		})
	}

	pub fn is_keygen_finished(&self) -> bool {
		let state = self.get_status();
		matches!(state, MetaHandlerStatus::Complete)
	}

	pub fn current_round_blame(&self) -> CurrentRoundBlame {
		self.current_round_blame.borrow().clone()
	}

	/// Setting this as the primary remote
	pub fn into_primary_remote(mut self) -> Self {
		self.is_primary_remote = true;
		self
	}
}

impl<C> Drop for AsyncProtocolRemote<C> {
	fn drop(&mut self) {
		if Arc::strong_count(&self.status) == 2 || self.is_primary_remote {
			// at this point, the only instances of this arc are this one, and,
			// presumably the one in the DKG worker. This one is asserted to be the one
			// belonging to the async proto. Signal as complete to allow the DKG worker to move
			// forward
			if self.get_status() != MetaHandlerStatus::Complete {
				log::info!(
					target: "dkg",
					"[drop code] MetaAsyncProtocol is ending: {:?}, History: {:?}",
					self.get_status(),
					self.status_history.lock(),
				);
				self.set_status(MetaHandlerStatus::Terminated);
			}

			let _ = self.shutdown("drop code");
		}
	}
}
