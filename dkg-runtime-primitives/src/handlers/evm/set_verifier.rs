use webb_proposals::SetVerifierProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<SetVerifierProposal, ValidationError> {
	let bytes: [u8; SetVerifierProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(SetVerifierProposal::from(bytes))
}

