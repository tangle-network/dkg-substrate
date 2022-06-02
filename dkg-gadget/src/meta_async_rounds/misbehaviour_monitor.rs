use std::future::Future;
use std::pin::Pin;
use sp_arithmetic::traits::AtLeast32BitUnsigned;
use dkg_primitives::types::DKGError;
use crate::meta_async_rounds::remote::MetaAsyncProtocolRemote;

/// The purpose of the misbehaviour monitor is to periodically check
/// the Meta handler to ensure that there are no misbehaving clients
pub struct MisbehaviourMonitor {
	inner: Pin<Box<dyn Future<Output=Result<(), DKGError>> + Send>>
}

impl MisbehaviourMonitor {
	pub fn new<C: AtLeast32BitUnsigned + Copy>(remote: MetaAsyncProtocolRemote<C>) -> Self {

	}
}
