use crate::dkg_modules::{
	KeygenProtocolSetupParameters, ProtocolInitReturn, SigningProtocolSetupParameters, DKG,
};
use async_trait::async_trait;
use dkg_primitives::types::DKGError;
use sp_runtime::traits::Block;

/// DKG module for Weighted Threshold Frost
pub struct WTFrostDKG {}

#[async_trait]
impl<B: Block> DKG<B> for WTFrostDKG {
	async fn initialize_keygen_protocol(
		&self,
		_params: KeygenProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>> {
		todo!()
	}

	async fn initialize_signing_protocol(
		&self,
		_params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError> {
		todo!()
	}

	fn can_handle_keygen_request(&self, params: &KeygenProtocolSetupParameters<B>) -> bool {
		matches!(params, KeygenProtocolSetupParameters::WTFrost { .. })
	}

	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool {
		matches!(params, SigningProtocolSetupParameters::WTFrost { .. })
	}
}
