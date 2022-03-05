use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader, ProposalNonce,
};
use codec::alloc::string::ToString;

pub struct ProposerSetUpdateProposal<C: ChainIdTrait> {
	pub merkle_root: Vec<u8>,        // 32 bytes
	pub average_session_length: u64, // 8 bytes
	pub chain_id: ChainIdType<C>,    // 6 bytes should be dummy zeroes
	pub nonce: ProposalNonce,        // 4 bytes
}

/// Proposal Data: [
///     merkle_root: 32 bytes
///     avarage_session_length: 8 bytes
///     chain_id: 6 bytes
///     nonce: 4 bytes
/// ]
/// Total Bytes: 32 + 8 + 6 + 4= 50 bytes
pub fn create<C: ChainIdTrait>(
	data: &[u8],
) -> Result<ProposerSetUpdateProposal<C>, ValidationError> {
	if data.len() != 50 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 50 bytes".to_string()))?
	}

	let mut merkle_root_bytes = [0u8; 32];
	merkle_root_bytes.copy_from_slice(&data[0..32]);
	let merkle_root = merkle_root_bytes.to_vec();

	let mut average_session_length_bytes = [0u8; 8];
	average_session_length_bytes.copy_from_slice(&data[32..40]);
	let average_session_length = u64::from_be_bytes(average_session_length_bytes);

	let mut chain_type_bytes = [0u8; 2];
	let mut chain_inner_id_bytes = [0u8; 4];
	chain_type_bytes.copy_from_slice(&data[40..42]);
	chain_inner_id_bytes.copy_from_slice(&data[42..46]);

	let chain_id = ChainIdType::from_raw_parts(chain_type_bytes, chain_inner_id_bytes);

	let mut nonce_bytes = [0u8; 4];
	nonce_bytes.copy_from_slice(&data[46..50]);
	let nonce = u32::from_be_bytes(nonce_bytes);

	Ok(ProposerSetUpdateProposal { merkle_root, average_session_length, chain_id, nonce })
}
