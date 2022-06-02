use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use futures::StreamExt;
use dkg_primitives::types::DKGError;
use crate::meta_async_rounds::blockchain_interface::BlockChainIface;
use crate::meta_async_rounds::dkg_gossip_engine::GossipEngineIface;
use crate::meta_async_rounds::remote::{MetaAsyncProtocolRemote, MetaHandlerStatus};

/// The purpose of the misbehaviour monitor is to periodically check
/// the Meta handler to ensure that there are no misbehaving clients
pub struct MisbehaviourMonitor {
	inner: Pin<Box<dyn Future<Output=Result<(), DKGError>> + Send>>
}

/// How frequently the misbehaviour monitor checks for misbehaving peers
pub const MISBEHAVIOUR_MONITOR_CHECK_INTERVAL: Duration = Duration::from_millis(2000);

impl MisbehaviourMonitor {
	pub fn new<BCIface: BlockChainIface + 'static>(remote: MetaAsyncProtocolRemote<BCIface::Clock>, bc_iface: Arc<BCIface>) -> Self {
		Self {
			inner: Box::pin(async move {
				let mut ticker = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(MISBEHAVIOUR_MONITOR_CHECK_INTERVAL));
				if let Some(gossip_engine) = bc_iface.get_gossip_engine() {
					while let Some(ins) = ticker.next().await {
						if let Some(ts) = gossip_engine.receive_timestamps() {
							match remote.get_status() {
								MetaHandlerStatus::Keygen => {
									if remote.keygen_has_stalled(bc_iface.now()) {
										// figure out who is stalling the keygen
										let lock = ts.read();
										for (peer, last_received_message_instant) in lock.iter() {

										}
									}
								}

								MetaHandlerStatus::OfflineAndVoting => {

								}

								_ => {
									// TODO: handle monitoring other stages
								}
							}
						}
					}
				}

				Err(DKGError::CriticalError { reason: "Misbehaviour monitor ended prematurely".to_string() })
			})
		}
	}
}

impl Future for MisbehaviourMonitor {
	type Output = Result<(), DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.inner.as_mut().poll(cx)
	}
}
