use codec::{Decode, Encode};
use dkg_primitives::types::DKGError;

mod dkg_signing_manager;

pub use dkg_signing_manager::DKGSigningManagerBuilder;
/// The Signing Manager is a simple trait that will be used
/// to sign some message. The Signed Message will be signed in async manner between the DKG Signers
/// and the result (the Signature) will be stored in the offchain storage.
#[async_trait::async_trait]
pub trait SigningManager {
	/// The type of messages that this Signing Manager is going to sign.
	type Message: Clone + Decode + Encode + Sync + Send + 'static;

	/// Try to sign this message, asynchronously, between the different DKG Signers.
	async fn sign(&self, msg: Self::Message) -> Result<(), DKGError>;

	/// Reset the Signing Manager to its initial state.
	/// and stop the worker.
	fn stop(&self);
}

/// A Dummy Signing Manager that does nothing.
pub struct DummySigner<M> {
	_marker: std::marker::PhantomData<M>,
}

impl<M> DummySigner<M> {
	pub fn new() -> Self {
		Self { _marker: std::marker::PhantomData }
	}
}

#[async_trait::async_trait]
impl<M> SigningManager for DummySigner<M>
where
	M: Clone + Decode + Encode + Sync + Send + 'static,
{
	type Message = M;

	async fn sign(&self, _msg: Self::Message) -> Result<(), DKGError> {
		Ok(())
	}

	fn stop(&self) {}
}
