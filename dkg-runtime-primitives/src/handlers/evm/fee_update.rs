use webb_proposals::WrappingFeeUpdateProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<WrappingFeeUpdateProposal, ValidationError> {
	let bytes: [u8; WrappingFeeUpdateProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(WrappingFeeUpdateProposal::from(bytes))
}

