use webb_proposals::ResourceIdUpdateProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<ResourceIdUpdateProposal, ValidationError> {
	let bytes: [u8; ResourceIdUpdateProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(ResourceIdUpdateProposal::from(bytes))
}
