use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ProposalHeader,
};
use codec::alloc::string::ToString;
use ethereum_types::{Address, U256};

pub struct RescueTokensProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub token_address: Address,
	pub to_address: Address,
	pub amount: U256,
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     tokenAddress        - 20 bytes  [40..60]
///     toAddress           - 20 bytes  [60..80]
///     amount              - 32 bytes  [80..112]
/// ]
/// Total Bytes: 32 + 4 + 4 + 20 + 20 + 32 = 112
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<RescueTokensProposal<C>, ValidationError> {
	if data.len() != 112 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 60 bytes".to_string()))?
	}
	let header: ProposalHeader<C> = decode_proposal_header(data)?;

	let mut token_address_bytes = [0u8; 20];
	token_address_bytes.copy_from_slice(&data[40..60]);
	let token_address = Address::from(token_address_bytes);

	let mut to_address_bytes = [0u8; 20];
	to_address_bytes.copy_from_slice(&data[60..80]);
	let to_address = Address::from(to_address_bytes);

	let mut amount_bytes = [0u8; 32];
	amount_bytes.copy_from_slice(&data[80..112]);
	let amount = U256::from(amount_bytes);

	// TODO: Add validation over EVM address
	Ok(RescueTokensProposal { header, token_address, to_address, amount })
}
