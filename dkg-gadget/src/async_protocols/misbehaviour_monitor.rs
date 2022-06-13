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

use crate::async_protocols::{
	blockchain_interface::BlockchainInterface, remote::MetaHandlerStatus, AsyncProtocolRemote,
};
use dkg_primitives::{
	crypto::Public,
	types::{DKGError, DKGMisbehaviourMessage, RoundId},
};
use dkg_runtime_primitives::MisbehaviourType;
use futures::StreamExt;
use std::{
	future::Future,
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};
use tokio::sync::mpsc::UnboundedSender;

/// The purpose of the misbehaviour monitor is to periodically check
/// the Meta handler to ensure that there are no misbehaving clients
pub struct MisbehaviourMonitor {
	inner: Pin<Box<dyn Future<Output = Result<(), DKGError>> + Send>>,
}

/// How frequently the misbehaviour monitor checks for misbehaving peers
pub const MISBEHAVIOUR_MONITOR_CHECK_INTERVAL: Duration = Duration::from_millis(2000);

impl MisbehaviourMonitor {
	pub fn new<BI: BlockchainInterface + 'static>(
		remote: AsyncProtocolRemote<BI::Clock>,
		bc_iface: BI,
		misbehaviour_tx: UnboundedSender<DKGMisbehaviourMessage>,
	) -> Self
	where
		BI::Clock: 'static,
	{
		Self {
			inner: Box::pin(async move {
				let mut ticker = tokio_stream::wrappers::IntervalStream::new(
					tokio::time::interval(MISBEHAVIOUR_MONITOR_CHECK_INTERVAL),
				);

				while (ticker.next().await).is_some() {
					log::trace!("[MisbehaviourMonitor] Performing periodic check ...");
					match remote.get_status() {
						MetaHandlerStatus::Keygen | MetaHandlerStatus::Complete => {
							if remote.keygen_has_stalled(bc_iface.now()) {
								on_keygen_timeout::<BI>(
									&remote,
									bc_iface.get_authority_set().as_slice(),
									&misbehaviour_tx,
									remote.round_id,
								)?
							}
						},
						MetaHandlerStatus::OfflineAndVoting => {},
						_ => {
							// TODO: handle monitoring other stages
						},
					}
				}

				Err(DKGError::CriticalError {
					reason: "Misbehaviour monitor ended prematurely".to_string(),
				})
			}),
		}
	}
}

pub fn on_keygen_timeout<BI: BlockchainInterface>(
	remote: &AsyncProtocolRemote<BI::Clock>,
	authority_set: &[Public],
	misbehaviour_tx: &UnboundedSender<DKGMisbehaviourMessage>,
	round_id: RoundId,
) -> Result<(), DKGError> {
	log::warn!("[MisbehaviourMonitor] Keygen has stalled! Will determine which authorities are misbehaving ...");
	let round_blame = remote.current_round_blame();
	log::warn!("[MisbehaviourMonitor] Current round blame: {:?}", round_blame);
	for party_i in round_blame.blamed_parties {
		// get the authority from the party index.
		authority_set
			.get(usize::from(party_i - 1))
			.cloned()
			.map(|offender| DKGMisbehaviourMessage {
				misbehaviour_type: MisbehaviourType::Keygen,
				round_id,
				offender,
				signature: vec![],
			})
			.and_then(|report| misbehaviour_tx.send(report).ok())
			.ok_or_else(|| DKGError::CriticalError {
				reason: format!("failed to report {party_i}"),
			})?;
		log::warn!("Blamed {party_i} for keygen stalled!");
	}

	Ok(())
}

impl Future for MisbehaviourMonitor {
	type Output = Result<(), DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.inner.as_mut().poll(cx)
	}
}
