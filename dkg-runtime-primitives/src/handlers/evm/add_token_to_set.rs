use webb_proposals::TokenAddProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<TokenAddProposal, ValidationError> {
	let bytes: [u8; TokenAddProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(TokenAddProposal::from(bytes))
}

