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
	blockchain_interface::BlockchainInterface, new_inner, remote::MetaHandlerStatus,
	state_machine::StateMachineHandler, AsyncProtocolParameters, GenericAsyncHandler, KeygenRound,
	ProtocolType,
};

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::{
	Error::ProceedRound, Keygen, ProceedError,
};

use std::fmt::Debug;

use dkg_primitives::types::{DKGError, DKGMsgStatus};
use futures::FutureExt;

impl<Out: Send + Debug + 'static> GenericAsyncHandler<'static, Out>
where
	(): Extend<Out>,
{
	/// Top-level function used to begin the execution of async protocols
	pub fn setup_keygen<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI>,
		threshold: u16,
		status: DKGMsgStatus,
	) -> Result<GenericAsyncHandler<'static, ()>, DKGError> {
		let status_handle = params.handle.clone();
		let mut stop_rx =
			status_handle.stop_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let start_rx =
			status_handle.start_rx.lock().take().ok_or_else(|| DKGError::GenericError {
				reason: "execute called twice with the same AsyncProtocol Parameters".to_string(),
			})?;

		let protocol = async move {
				dkg_logging::info!(target: "dkg_gadget::keygen", "Will execute keygen since local is in best authority set");
				let t = threshold;
				let n = params.best_authorities.len() as u16;
				// wait for the start signal
				start_rx
					.await
					.map_err(|err| DKGError::StartKeygen { reason: err.to_string() })?;
				// Set status of the handle
				params.handle.set_status(MetaHandlerStatus::Keygen);
				// Execute the keygen
				GenericAsyncHandler::new_keygen(params, t, n, status)?.await?;
				dkg_logging::debug!(target: "dkg_gadget::keygen", "Keygen stage complete!");

			Ok(())
		}
		.then(|res| async move {
			match res {
				Ok(_) => {
					// Set the status as complete.
					status_handle.set_status(MetaHandlerStatus::Complete);
					dkg_logging::info!(target: "dkg_gadget::keygen", "🕸️  Keygen GenericAsyncHandler completed");
				}
				Err(ref err) => {
					// Do not update the status here, evetually the Keygen will fail and timeout.
					dkg_logging::error!(target: "dkg_gadget::keygen", "Keygen failed with error: {:?}", err);
				}
			};
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					dkg_logging::info!(target: "dkg_gadget::keygen", "Stopper has been called {:?}", res1);
					Ok(())
				}
			}
		});

		Ok(GenericAsyncHandler { protocol })
	}

	fn new_keygen<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI>,
		t: u16,
		n: u16,
		status: DKGMsgStatus,
	) -> Result<GenericAsyncHandler<'static, <Keygen as StateMachineHandler>::Return>, DKGError> {
		let ty = match status {
			DKGMsgStatus::ACTIVE => KeygenRound::ACTIVE,
			DKGMsgStatus::QUEUED => KeygenRound::QUEUED,
		};
		let i = params.party_i;
		let channel_type = ProtocolType::Keygen { ty, i, t, n };
		new_inner(
			(),
			Keygen::new(i, t, n).map_err(|err| Self::map_keygen_error_to_dkg_error(err))?,
			params,
			channel_type,
			0,
			status,
		)
	}

	fn map_keygen_error_to_dkg_error(
		error : multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Error,
	) -> DKGError {
		match error {
			// extract the bad actors from error messages
			ProceedRound(ProceedError::Round2VerifyCommitments(e)) =>
				DKGError::KeygenMisbehaviour {
					reason: e.error_type.to_string(),
					bad_actors: e.bad_actors,
				},
			ProceedRound(ProceedError::Round3VerifyVssConstruct(e)) =>
				DKGError::KeygenMisbehaviour {
					reason: e.error_type.to_string(),
					bad_actors: e.bad_actors,
				},
			ProceedRound(ProceedError::Round4VerifyDLogProof(e)) => DKGError::KeygenMisbehaviour {
				reason: e.error_type.to_string(),
				bad_actors: e.bad_actors,
			},
			_ => DKGError::KeygenMisbehaviour { reason: error.to_string(), bad_actors: vec![] },
		}
	}
}
