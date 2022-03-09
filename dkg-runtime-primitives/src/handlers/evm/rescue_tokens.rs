use webb_proposals::RescueTokensProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<RescueTokensProposal, ValidationError> {
	let bytes: [u8; RescueTokensProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(RescueTokensProposal::from(bytes))
}

