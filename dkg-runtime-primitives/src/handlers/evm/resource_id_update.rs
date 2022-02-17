use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader,
};
use codec::alloc::string::ToString;
use ethereum_types::Address;

pub struct ResourceIdUpdateProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub new_resource_id: [u8; 32],
	pub handler_address: Address,
	pub execution_address: Address,
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     resourceId          - 32 bytes  [40..72]
///     handlerAddress      - 20 bytes  [72..92]
///     executionAddress    - 20 bytes  [92..112]
/// ]
/// Total Bytes: 32 + 4 + 4 + 32 + 20 + 20 = 112
pub fn create<C: ChainIdTrait>(
	data: &[u8],
) -> Result<ResourceIdUpdateProposal<C>, ValidationError> {
	if data.len() != 112 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 60 bytes".to_string()))?
	}
	let header: ProposalHeader<C> = decode_proposal_header(data)?;

	let mut new_resource_id = [0u8; 32];
	new_resource_id.copy_from_slice(&data[40..72]);

	let mut handler_address_bytes = [0u8; 20];
	handler_address_bytes.copy_from_slice(&data[72..92]);
	let handler_address = Address::from(handler_address_bytes);

	let mut execution_address_bytes = [0u8; 20];
	execution_address_bytes.copy_from_slice(&data[92..112]);
	let execution_address = Address::from(execution_address_bytes);

	// TODO: Add validation over EVM address
	Ok(ResourceIdUpdateProposal { header, new_resource_id, handler_address, execution_address })
}
