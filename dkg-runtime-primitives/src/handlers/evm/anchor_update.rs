use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ProposalHeader,
};
use codec::alloc::string::ToString;

pub struct AnchorUpdateProposal<C: ChainIdTrait> {
	pub header: ProposalHeader<C>,
	pub src_chain_id: u64,
	pub latest_leaf_index: u32,
	pub merkle_root: [u8; 32],
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     srcChainId          - 6 bytes  [40..46]
///     latestLeafIndex     - 4 bytes  [46..50]
///     merkleRoot          - 32 bytes [50..82]
/// ]
/// Total Bytes: 32 + 4 + 4 + 6 + 4 + 32 = 82
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<AnchorUpdateProposal<C>, ValidationError> {
	if data.len() != 82 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 82 bytes".to_string()))
	}
	let header: ProposalHeader<C> = decode_proposal_header(data)?;

	let mut src_chain_id_bytes = [0u8; 8];
	src_chain_id_bytes[2..8].copy_from_slice(&data[40..46]);
	let src_chain_id = u64::from_be_bytes(src_chain_id_bytes);

	let mut latest_leaf_index_bytes = [0u8; 4];
	latest_leaf_index_bytes.copy_from_slice(&data[46..50]);
	let latest_leaf_index = u32::from_be_bytes(latest_leaf_index_bytes);

	let mut merkle_root = [0u8; 32];
	merkle_root.copy_from_slice(&data[50..82]);
	// TODO: Ensure function sig is non-zero
	Ok(AnchorUpdateProposal { header, src_chain_id, latest_leaf_index, merkle_root })
}
