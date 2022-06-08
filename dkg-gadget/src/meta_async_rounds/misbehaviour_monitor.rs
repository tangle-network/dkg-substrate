use crate::meta_async_rounds::{
	blockchain_interface::BlockChainIface,
	remote::{MetaAsyncProtocolRemote, MetaHandlerStatus},
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
	pub fn new<BCIface: BlockChainIface + 'static>(
		remote: MetaAsyncProtocolRemote<BCIface::Clock>,
		bc_iface: BCIface,
		misbehaviour_tx: UnboundedSender<DKGMisbehaviourMessage>,
	) -> Self
	where
		BCIface::Clock: 'static,
	{
		Self {
			inner: Box::pin(async move {
				let mut ticker = tokio_stream::wrappers::IntervalStream::new(
					tokio::time::interval(MISBEHAVIOUR_MONITOR_CHECK_INTERVAL),
				);

				while let Some(_) = ticker.next().await {
					log::info!("[MisbehaviourMonitor] Performing periodic check ...");
					match remote.get_status() {
						MetaHandlerStatus::Keygen | MetaHandlerStatus::Complete => {
							if remote.keygen_has_stalled(bc_iface.now()) {
								on_keygen_timeout::<BCIface>(
									&remote,
									bc_iface.get_authority_set().as_slice(),
									&misbehaviour_tx,
									remote.round_id,
								)?
							}

							if remote.get_status() == MetaHandlerStatus::Complete {
								// when the primary remote drops, the status will be flipped to
								// Complete
								log::info!("[MisbehaviourMonitor] Ending since the corresponding MetaAsyncProtocolHandler has ended");
								return Ok(())
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

pub fn on_keygen_timeout<BCIface: BlockChainIface>(
	remote: &MetaAsyncProtocolRemote<BCIface::Clock>,
	authority_set: &[Public],
	misbehaviour_tx: &UnboundedSender<DKGMisbehaviourMessage>,
	round_id: RoundId,
) -> Result<(), DKGError> {
	log::warn!("[MisbehaviourMonitor] Keygen has stalled! Will determine which authorities are misbehaving ...");
	let round_blame = remote.current_round_blame();
	for party_i in round_blame.blamed_parties {
		authority_set
			.get(usize::from(party_i))
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
	}

	Ok(())
}

impl Future for MisbehaviourMonitor {
	type Output = Result<(), DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.inner.as_mut().poll(cx)
	}
}
