use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use futures::StreamExt;
use itertools::Itertools;
use sc_client_api::Backend;
use sc_network::PeerId;
use sp_runtime::traits::{Block, NumberFor};
use dkg_primitives::types::{DKGError, DKGMisbehaviourMessage, RoundId};
use dkg_runtime_primitives::crypto::AuthorityId;
use dkg_runtime_primitives::MisbehaviourType;
use crate::Client;
use crate::messages::misbehaviour_report::gossip_misbehaviour_report;
use crate::meta_async_rounds::blockchain_interface::BlockChainIface;
use crate::meta_async_rounds::dkg_gossip_engine::{GossipEngineIface, ReceiveTimestamp};
use crate::meta_async_rounds::remote::{MetaAsyncProtocolRemote, MetaHandlerStatus};
use crate::worker::{HasAggregatedMisbehaviourReports, HasBackend, HasClient, HasGossipEngine, HasLatestHeader, KeystoreExt};

/// The purpose of the misbehaviour monitor is to periodically check
/// the Meta handler to ensure that there are no misbehaving clients
pub struct MisbehaviourMonitor {
	inner: Pin<Box<dyn Future<Output=Result<(), DKGError>> + Send>>
}

/// How frequently the misbehaviour monitor checks for misbehaving peers
pub const MISBEHAVIOUR_MONITOR_CHECK_INTERVAL: Duration = Duration::from_millis(2000);

impl MisbehaviourMonitor {
	pub fn new<
		B: Block,
		BE: Backend<B>,
		C: Client<B, BE>,
		GE: GossipEngineIface,
		BCIface: KeystoreExt + HasBackend<B, BE> + HasLatestHeader<B> + HasClient<B, BE, C> + HasAggregatedMisbehaviourReports + HasGossipEngine<GE>>(remote: MetaAsyncProtocolRemote<NumberFor<B>>, bc_iface: BCIface) -> Self {
		Self {
			inner: Box::pin(async move {
				let mut ticker = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(MISBEHAVIOUR_MONITOR_CHECK_INTERVAL));
				let gossip_engine = bc_iface.get_gossip_engine();

				while let Some(_) = ticker.next().await {
					log::info!("[MisbehaviourMonitor] Performing periodic check ...");
					if let Some(ts) = gossip_engine.receive_timestamps() {
						match remote.get_status() {
							MetaHandlerStatus::Keygen | MetaHandlerStatus::Complete => {
								if remote.keygen_has_stalled(bc_iface.now()) {
									on_keygen_timeout(&bc_iface, ts, remote.round_id)
								}

								if remote.get_status() == MetaHandlerStatus::Complete {
									// when the primary remote drops, the status will be flipped to Complete
									log::info!("[MisbehaviourMonitor] Ending since the corresponding MetaAsyncProtocolHandler has ended");
									return Ok(())
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

				Err(DKGError::CriticalError { reason: "Misbehaviour monitor ended prematurely".to_string() })
			})
		}
	}
}

pub fn on_keygen_timeout<
	B: Block,
	BE: Backend<B>,
	C: Client<B, BE>,
	GE: GossipEngineIface,
	BCIface: KeystoreExt + HasBackend<B, BE> + HasLatestHeader<B> + HasClient<B, BE, C> + HasAggregatedMisbehaviourReports + HasGossipEngine<GE>>(bc_iface: &BCIface, ts: &ReceiveTimestamp<NumberFor<B>>, round_id: RoundId) {
	log::warn!("[MisbehaviourMonitor] Keygen has stalled! Will determine which authorities are misbehaving ...");
	// figure out who is stalling the keygen
	let lock = ts.read();
	if lock.len() > 0 {
		// find the bottleneck (the slowest user)
		let (slowest_user, (_, _, offender)) = lock.iter()
			.sorted_by(|(_peer_id, (_, t0, _)), (_peer_id2, (_, t1, _))| {
				t0.cmp(t1)
			}).next().unwrap();

		let report = DKGMisbehaviourMessage {
			misbehaviour_type: MisbehaviourType::Keygen,
			round_id,
			offender: offender.clone(),
			signature: vec![]
		};

		gossip_misbehaviour_report(bc_iface, report)
	}
}

impl Future for MisbehaviourMonitor {
	type Output = Result<(), DKGError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.inner.as_mut().poll(cx)
	}
}
