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

use crate::async_protocols::remote::ShutdownReason;
use dkg_primitives::types::DKGError;
use dkg_runtime_primitives::MaxAuthorities;
use futures::FutureExt;

impl<Out: Send + Debug + 'static> GenericAsyncHandler<'static, Out>
where
	(): Extend<Out>,
{
	/// Top-level function used to begin the execution of async protocols
	pub fn setup_keygen<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		threshold: u16,
		status: KeygenRound,
		keygen_protocol_hash: [u8; 32],
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

		let logger0 = params.logger.clone();
		let logger1 = params.logger.clone();

		let protocol = async move {
			params.logger.info_keygen(
				"Will execute keygen since local is in best authority set".to_string(),
			);
			let t = threshold;
			let n = params.best_authorities.len() as u16;
			// wait for the start signal
			start_rx
				.await
				.map_err(|err| DKGError::StartKeygen { reason: err.to_string() })?;
			// Set status of the handle
			params.handle.set_status(MetaHandlerStatus::Keygen);
			// Execute the keygen
			GenericAsyncHandler::new_keygen(params.clone(), t, n, status, keygen_protocol_hash)?
				.await?;
			params.logger.debug_keygen("Keygen stage complete!");

			Ok(())
		}
		.then(|res| async move {
			match res {
				Ok(_) => {
					// Set the status as complete.
					status_handle.set_status(MetaHandlerStatus::Complete);
					logger0.info_keygen("🕸️  Keygen GenericAsyncHandler completed".to_string());
				},
				Err(ref err) => {
					// Do not update the status here, eventually the Keygen will fail and timeout.
					logger0.error_keygen(format!("Keygen failed with error: {err:?}"));
				},
			};
			res
		});

		let protocol = Box::pin(async move {
			tokio::select! {
				res0 = protocol => res0,
				res1 = stop_rx.recv() => {
					logger1.info_keygen(format!("Stopper has been called {res1:?}"));
					if let Some(res1) = res1 {
						if res1 == ShutdownReason::DropCode {
							Ok(())
						} else {
							Err(DKGError::GenericError { reason: "Keygen has stalled".into() })
						}
					} else {
						Ok(())
					}
				}
			}
		});

		Ok(GenericAsyncHandler { protocol })
	}

	fn new_keygen<BI: BlockchainInterface + 'static>(
		params: AsyncProtocolParameters<BI, MaxAuthorities>,
		t: u16,
		n: u16,
		ty: KeygenRound,
		keygen_protocol_hash: [u8; 32],
	) -> Result<GenericAsyncHandler<'static, <Keygen as StateMachineHandler<BI>>::Return>, DKGError>
	{
		let i = params.party_i;
		let associated_round_id = params.associated_block_id;
		let channel_type: ProtocolType<
			<BI as BlockchainInterface>::BatchId,
			<BI as BlockchainInterface>::MaxProposalLength,
			<BI as BlockchainInterface>::MaxProposalsInBatch,
			<BI as BlockchainInterface>::Clock,
		> = ProtocolType::Keygen {
			ty,
			i,
			t,
			n,
			associated_block_id: associated_round_id,
			keygen_protocol_hash,
		};
		new_inner(
			(),
			Keygen::new(*i.as_ref(), t, n)
				.map_err(|err| Self::map_keygen_error_to_dkg_error_keygen(err))?,
			params,
			channel_type,
		)
	}

	fn map_keygen_error_to_dkg_error_keygen(
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
