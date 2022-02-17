use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader, Vec,
};
use codec::alloc::string::ToString;
use ethereum_types::Address;

pub struct RemoveTokenProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub encoded_call: Vec<u8>,
}

/// https://github.com/webb-tools/protocol-substrate/issues/142
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     zeroes              - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     call                -          [40..]
/// ]
/// Total Bytes: 32 + 4 + 4 + (size of call) = 40 + (size of call)
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<RemoveTokenProposal<C>, ValidationError> {
	let header: ProposalHeader<C> = decode_proposal_header(data)?;
	let zeroes = header.function_sig;
	// Check that zeroes is actually zero
	if u32::from_be_bytes(zeroes) != 0 {
		return Err(ValidationError::InvalidParameter("Function Sig should be zero".to_string()))?
	}
	let encoded_call = data[40..].to_vec();
	Ok(RemoveTokenProposal { header, encoded_call })
}
