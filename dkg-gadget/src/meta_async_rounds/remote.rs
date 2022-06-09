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

use atomic::Atomic;
use core::fmt;
use dkg_primitives::types::{DKGError, RoundId, SignedDKGMessage};
use dkg_runtime_primitives::{crypto::Public, UnsignedProposal, KEYGEN_TIMEOUT};
use parking_lot::Mutex;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc::error::SendError;

use super::meta_handler::CurrentRoundBlame;

pub(crate) type UnsignedProposalsSender =
	tokio::sync::mpsc::UnboundedSender<Option<Vec<UnsignedProposal>>>;
pub(crate) type UnsignedProposalsReceiver =
	Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Option<Vec<UnsignedProposal>>>>>>;

#[derive(Clone)]
pub struct MetaAsyncProtocolRemote<C> {
	status: Arc<Atomic<MetaHandlerStatus>>,
	unsigned_proposals_tx: UnsignedProposalsSender,
	pub(crate) unsigned_proposals_rx: UnsignedProposalsReceiver,
	pub(crate) broadcaster: tokio::sync::broadcast::Sender<Arc<SignedDKGMessage<Public>>>,
	start_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
	pub(crate) start_rx: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
	stop_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>>,
	pub(crate) stop_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
	started_at: C,
	is_primary_remote: bool,
	current_round_blame: tokio::sync::watch::Receiver<CurrentRoundBlame>,
	pub(crate) current_round_blame_tx: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
	pub(crate) round_id: RoundId,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MetaHandlerStatus {
	Beginning,
	Keygen,
	AwaitingProposals,
	OfflineAndVoting,
	Complete,
	Timeout,
}

impl<C: AtLeast32BitUnsigned + Copy> MetaAsyncProtocolRemote<C> {
	/// Create at the beginning of each meta handler instantiation
	pub fn new(at: C, round_id: RoundId) -> Self {
		let (unsigned_proposals_tx, unsigned_proposals_rx) = tokio::sync::mpsc::unbounded_channel();
		let (stop_tx, stop_rx) = tokio::sync::mpsc::unbounded_channel();
		let (broadcaster, _) = tokio::sync::broadcast::channel(4096);
		let (start_tx, start_rx) = tokio::sync::oneshot::channel();

		let (current_round_blame_tx, current_round_blame) =
			tokio::sync::watch::channel(CurrentRoundBlame::empty());

		Self {
			status: Arc::new(Atomic::new(MetaHandlerStatus::Beginning)),
			unsigned_proposals_tx,
			unsigned_proposals_rx: Arc::new(Mutex::new(Some(unsigned_proposals_rx))),
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

	pub fn keygen_has_stalled(&self, now: C) -> bool
	where
		C: fmt::Debug,
	{
		let status = self.get_status();
		(status == MetaHandlerStatus::Keygen || status == MetaHandlerStatus::Timeout) &&
			(now - self.started_at > KEYGEN_TIMEOUT.into())
	}
}

impl<C> MetaAsyncProtocolRemote<C> {
	pub fn get_status(&self) -> MetaHandlerStatus {
		self.status.load(Ordering::SeqCst)
	}

	pub fn set_status(&self, status: MetaHandlerStatus) {
		self.status.store(status, Ordering::SeqCst)
	}

	pub fn is_active(&self) -> bool {
		let status = self.get_status();
		status != MetaHandlerStatus::Beginning && status != MetaHandlerStatus::Complete
	}

	pub fn submit_unsigned_proposals(
		&self,
		unsigned_proposals: Vec<UnsignedProposal>,
	) -> Result<(), SendError<Option<Vec<UnsignedProposal>>>> {
		if unsigned_proposals.is_empty() {
			log::trace!("[{}] No unsigned proposals to submit", self.round_id);
			return Ok(())
		}
		log::info!(target: "dkg", "Sending unsigned proposals: {:?}", unsigned_proposals);
		self.unsigned_proposals_tx.send(Some(unsigned_proposals))
	}

	#[allow(dead_code)]
	pub fn end_unsigned_proposals(&self) -> Result<(), SendError<Option<Vec<UnsignedProposal>>>> {
		self.unsigned_proposals_tx.send(None)
	}

	pub fn deliver_message(
		&self,
		msg: Arc<SignedDKGMessage<Public>>,
	) -> Result<(), tokio::sync::broadcast::error::SendError<Arc<SignedDKGMessage<Public>>>> {
		if self.broadcaster.receiver_count() != 0 {
			self.broadcaster.send(msg).map(|_| ())
		} else {
			// do not forward the message
			log::debug!(target: "dkg", "Will not deliver message since there are no receivers");
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
	pub fn shutdown(&self) -> Result<(), DKGError> {
		let tx = self.stop_tx.lock().take().ok_or_else(|| DKGError::GenericError {
			reason: "Shutdown has already been called".to_string(),
		})?;
		tx.send(()).map_err(|_| DKGError::GenericError {
			reason: "Unable to send shutdown signal (already shut down?)".to_string(),
		})
	}

	pub fn is_keygen_finished(&self) -> bool {
		let state = self.get_status();
		matches!(
			state,
			MetaHandlerStatus::AwaitingProposals |
				MetaHandlerStatus::OfflineAndVoting |
				MetaHandlerStatus::Complete
		)
	}

	/// Setting this as the primary remote
	pub fn into_primary_remote(mut self) -> Self {
		self.is_primary_remote = true;
		self
	}

	pub fn current_round_blame(&self) -> CurrentRoundBlame {
		self.current_round_blame.borrow().clone()
	}
}

impl<C> Drop for MetaAsyncProtocolRemote<C> {
	fn drop(&mut self) {
		if Arc::strong_count(&self.status) == 2 || self.is_primary_remote {
			// at this point, the only instances of this arc are this one, and,
			// presumably the one in the DKG worker. This one is asserted to be the one
			// belonging to the async proto. Signal as complete to allow the DKG worker to move
			// forward
			if self.get_status() != MetaHandlerStatus::Complete &&
				self.get_status() != MetaHandlerStatus::Timeout
			{
				log::info!(target: "dkg", "[drop code] MetaAsyncProtocol is ending");
				self.set_status(MetaHandlerStatus::Complete);
			}

			let _ = self.shutdown();
		}
	}
}
