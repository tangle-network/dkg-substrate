use webb_proposals::AnchorUpdateProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<AnchorUpdateProposal, ValidationError> {
	let bytes: [u8; AnchorUpdateProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(AnchorUpdateProposal::from(bytes))
}

