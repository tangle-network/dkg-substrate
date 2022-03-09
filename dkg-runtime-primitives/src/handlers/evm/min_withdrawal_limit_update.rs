use webb_proposals::MinWithdrawalLimitProposal;

use crate::handlers::validate_proposals::ValidationError;

pub fn create(data: &[u8]) -> Result<MinWithdrawalLimitProposal, ValidationError> {
	let bytes: [u8; MinWithdrawalLimitProposal::LENGTH] =
		data.try_into().map_err(|_| ValidationError::InvalidProposalBytesLength)?;
	Ok(MinWithdrawalLimitProposal::from(bytes))
}
