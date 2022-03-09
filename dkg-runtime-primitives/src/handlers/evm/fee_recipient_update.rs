use webb_proposals::FeeRecipientUpdateProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<FeeRecipientUpdateProposal, ValidationError> {
	let bytes: [u8; FeeRecipientUpdateProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(FeeRecipientUpdateProposal::from(bytes))
}

