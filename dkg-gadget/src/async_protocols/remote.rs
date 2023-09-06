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

use crate::{
	async_protocols::CurrentRoundBlame, debug_logger::DebugLogger, worker::ProtoStageType,
};
use atomic::Atomic;
use dkg_primitives::types::{DKGError, NetworkMsgPayload, SessionId, SignedDKGMessage, SSID};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	KEYGEN_TIMEOUT, SIGN_TIMEOUT,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use std::{
	collections::HashMap,
	sync::{atomic::Ordering, Arc},
};

pub struct AsyncProtocolRemote<C> {
	pub(crate) status: Arc<Atomic<MetaHandlerStatus>>,
	tx_keygen_signing: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<Public>>,
	tx_voting: tokio::sync::mpsc::UnboundedSender<SignedDKGMessage<Public>>,
	pub(crate) rx_keygen_signing: MessageReceiverHandle,
	pub(crate) rx_voting: MessageReceiverHandle,
	start_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
	pub(crate) start_rx: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
	stop_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<ShutdownReason>>>>,
	pub(crate) stop_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<ShutdownReason>>>>,
	pub(crate) started_at: C,
	pub(crate) is_primary_remote: bool,
	pub(crate) current_round_blame: tokio::sync::watch::Receiver<CurrentRoundBlame>,
	pub(crate) current_round_blame_tx: Arc<tokio::sync::watch::Sender<CurrentRoundBlame>>,
	pub(crate) session_id: SessionId,
	pub(crate) associated_block_id: u64,
	/// The signing set index. For keygen, this is always 0
	pub(crate) ssid: SSID,
	pub(crate) logger: DebugLogger,
	status_history: Arc<Mutex<Vec<MetaHandlerStatus>>>,
	/// Contains the mapping of index to authority id for this specific protocol. Varies between
	/// protocols
	pub(crate) index_to_authority_mapping: Arc<HashMap<usize, AuthorityId>>,
	pub(crate) proto_stage_type: ProtoStageType,
}

type MessageReceiverHandle =
	Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<SignedDKGMessage<Public>>>>>;

impl<C: Clone> Clone for AsyncProtocolRemote<C> {
	fn clone(&self) -> Self {
		let strong_count_next = Arc::strong_count(&self.status) + 1;
		let status = self.get_status();
		debug_log_cloning_and_dropping(
			&self.logger,
			"CLONE",
			strong_count_next,
			self.session_id,
			status,
		);

		Self {
			status: self.status.clone(),
			tx_keygen_signing: self.tx_keygen_signing.clone(),
			tx_voting: self.tx_voting.clone(),
			rx_keygen_signing: self.rx_keygen_signing.clone(),
			rx_voting: self.rx_voting.clone(),
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
			associated_block_id: self.associated_block_id,
			ssid: self.ssid,
			proto_stage_type: self.proto_stage_type,
			index_to_authority_mapping: self.index_to_authority_mapping.clone(),
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ShutdownReason {
	Stalled,
	DropCode,
}

impl<C: AtLeast32BitUnsigned + Copy + Send> AsyncProtocolRemote<C> {
	/// Create at the beginning of each meta handler instantiation
	pub fn new(
		at: C,
		session_id: SessionId,
		logger: DebugLogger,
		associated_block_id: u64,
		ssid: SSID,
		proto_stage_type: ProtoStageType,
		index_to_authority_mapping: Arc<HashMap<usize, AuthorityId>>,
	) -> Self {
		let (stop_tx, stop_rx) = tokio::sync::mpsc::unbounded_channel();
		let (tx_keygen_signing, rx_keygen_signing) = tokio::sync::mpsc::unbounded_channel();
		let (tx_voting, rx_voting) = tokio::sync::mpsc::unbounded_channel();
		let (start_tx, start_rx) = tokio::sync::oneshot::channel();

		let (current_round_blame_tx, current_round_blame) =
			tokio::sync::watch::channel(CurrentRoundBlame::empty());

		let status = Arc::new(Atomic::new(MetaHandlerStatus::Beginning));
		let status_history = Arc::new(Mutex::new(vec![MetaHandlerStatus::Beginning]));

		Self {
			status,
			tx_keygen_signing,
			tx_voting,
			proto_stage_type,
			rx_keygen_signing: Arc::new(Mutex::new(Some(rx_keygen_signing))),
			rx_voting: Arc::new(Mutex::new(Some(rx_voting))),
			status_history,
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
			associated_block_id,
			ssid,
			index_to_authority_mapping,
		}
	}

	pub fn set_as_primary(&mut self) {
		self.is_primary_remote = true;
	}

	pub fn keygen_has_stalled(&self, now: C) -> bool {
		self.has_stalled(now, KEYGEN_TIMEOUT)
	}

	pub fn signing_has_stalled(&self, now: C) -> bool {
		self.has_stalled(now, SIGN_TIMEOUT)
	}

	fn has_stalled(&self, now: C, timeout: u32) -> bool {
		let state = self.get_status();

		// if the state is terminated, preemptively assume we are stalled
		// to allow other tasks to take this one's place
		if state == MetaHandlerStatus::Terminated {
			return true
		}

		// otherwise, if we have timed-out, no matter the state, we are stalled
		now >= self.started_at + timeout.into()
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

	#[allow(clippy::result_large_err)]
	pub fn deliver_message(
		&self,
		msg: SignedDKGMessage<Public>,
	) -> Result<(), tokio::sync::mpsc::error::SendError<SignedDKGMessage<Public>>> {
		let status = self.get_status();
		let can_deliver =
			status != MetaHandlerStatus::Complete && status != MetaHandlerStatus::Terminated;
		if can_deliver {
			if matches!(msg.msg.payload, NetworkMsgPayload::Vote(..)) {
				self.tx_voting.send(msg)
			} else {
				self.tx_keygen_signing.send(msg)
			}
		} else {
			self.logger.warn(format!(
				"Did not deliver message due to state {status:?} {:?}",
				msg.msg.payload
			));
			Ok(())
		}
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
	pub fn shutdown(&self, reason: ShutdownReason) -> Result<(), DKGError> {
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
		self.logger.warn(format!("Shutting down meta handler: {reason:?}"));
		tx.send(reason).map_err(|_| DKGError::GenericError {
			reason: "Unable to send shutdown signal (already shut down?)".to_string(),
		})
	}

	pub fn is_keygen_finished(&self) -> bool {
		self.is_completed()
	}

	pub fn has_started(&self) -> bool {
		self.get_status() != MetaHandlerStatus::Beginning
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
		let strong_count_next = Arc::strong_count(&self.status).saturating_sub(1);
		let status = self.get_status();
		debug_log_cloning_and_dropping(
			&self.logger,
			"DROP",
			strong_count_next,
			self.session_id,
			status,
		);

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

			let _ = self.shutdown(ShutdownReason::DropCode);
		}
	}
}

fn debug_log_cloning_and_dropping(
	logger: &DebugLogger,
	prepend_tag: &str,
	next_arc_strong_count: usize,
	session_id: SessionId,
	status: MetaHandlerStatus,
) {
	logger.debug(format!(
		"{prepend_tag}: next_arc_strong_count: {next_arc_strong_count}, session_id: {session_id} | status: {status:?}",
	));
}
