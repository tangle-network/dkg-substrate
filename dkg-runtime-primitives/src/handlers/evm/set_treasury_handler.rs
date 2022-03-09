use webb_proposals::SetTreasuryHandlerProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<SetTreasuryHandlerProposal, ValidationError> {
	let bytes: [u8; SetTreasuryHandlerProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(SetTreasuryHandlerProposal::from(bytes))
}

