use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader,
};
use codec::alloc::string::ToString;
use ethereum_types::Address;

pub struct FeeUpdateProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub fee: u8,
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     fee                 - 1 bytes  [40..41]
/// ]
/// Total Bytes: 32 + 4 + 4 + 1 = 41
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<FeeUpdateProposal<C>, ValidationError> {
	if data.len() != 41 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 41 bytes".to_string()))?
	}
	let header: ProposalHeader<C> = decode_proposal_header(data)?;

	let fee = data[40];
	// TODO: Add validation over EVM address
	Ok(FeeUpdateProposal { header, fee })
}
