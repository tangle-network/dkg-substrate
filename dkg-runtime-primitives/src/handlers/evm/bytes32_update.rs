use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader,
};
use codec::alloc::string::ToString;
use ethereum_types::Address;

pub struct Bytes32UpdateProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub target: [u8; 32],
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     target              - 32 bytes  [40..72]
/// ]
/// Total Bytes: 32 + 4 + 4 + 32 = 72
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<Bytes32UpdateProposal<C>, ValidationError> {
	if data.len() != 72 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 72 bytes".to_string()))?
	}
	let header: ProposalHeader<C> = decode_proposal_header(data)?;

	let mut target = [0u8; 32];
	target.copy_from_slice(&data[40..]);
	// TODO: Add validation over EVM address
	Ok(Bytes32UpdateProposal { header, target })
}
