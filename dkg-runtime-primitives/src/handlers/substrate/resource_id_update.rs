use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	Vec,
};
use codec::alloc::string::ToString;

pub struct ResourceIdProposal {
	pub header: webb_proposals::ProposalHeader,
	pub r_id_to_register: [u8; 32],
	pub method_name: Vec<u8>,
}

/// https://github.com/webb-tools/protocol-substrate/issues/142
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     zeroes              - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     r_id_to_register    - 32 bytes [40..72]
///     method_name         - [72..]
/// ]
/// Total Bytes: 32 + 4 + 4 + 32 + (size of method name) = 72 + (size of method name)
pub fn create(data: &[u8]) -> Result<ResourceIdProposal, ValidationError> {
	if data.len() < 72 {
		return Err(ValidationError::InvalidProposalBytesLength)
	}
	let header = decode_proposal_header(data)?;
	let zeroes = header.function_signature().to_bytes();
	// Check that zeroes is actually zero
	if u32::from_be_bytes(zeroes) != 0 {
		return Err(ValidationError::InvalidParameter("Function Sig should be zero".to_string()))
	}

	let mut r_id_to_register = [0u8; 32];
	r_id_to_register.copy_from_slice(&data[40..72]);
	let method_name = data[72..].to_vec();
	Ok(ResourceIdProposal { header, r_id_to_register, method_name })
}
