use webb_proposals::MaxDepositLimitProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<MaxDepositLimitProposal, ValidationError> {
	let bytes: [u8; MaxDepositLimitProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(MaxDepositLimitProposal::from(bytes))
}
