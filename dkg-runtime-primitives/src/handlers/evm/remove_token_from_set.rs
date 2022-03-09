use webb_proposals::TokenRemoveProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<TokenRemoveProposal, ValidationError> {
	let bytes: [u8; TokenRemoveProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(TokenRemoveProposal::from(bytes))
}
