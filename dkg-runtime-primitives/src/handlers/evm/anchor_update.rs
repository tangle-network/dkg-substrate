use sp_std::vec::Vec;

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     srcChainId          - 4 bytes  [40..44]
///     latestLeafIndex     - 4 bytes  [44..48]
///     merkleRoot          - 32 bytes [48..80]
/// ]
/// Total Bytes: 32 + 4 + 4 + 4 + 4 + 32 = 80
pub fn validate_anchor_update(data: &[u8]) -> bool {
    if data.len() != 80 {
        return false;
    }
    let mut src_chain_id_bytes = [0u8; 4];
    src_chain_id_bytes.copy_from_slice(&data[40..44]);
    let src_chain_id = u32::from_be_bytes(src_chain_id_bytes);
    let mut latest_leaf_index_bytes = [0u8; 4];
    latest_leaf_index_bytes.copy_from_slice(&data[44..48]);
    let latest_leaf_index = u32::from_be_bytes(latest_leaf_index_bytes);
    let mut merkle_root_bytes = [0u8; 32];
    merkle_root_bytes.copy_from_slice(&data[48..80]);
    // TODO: Ensure function sig is non-zero
    return true;
}

/// Creates an anchor update
pub fn create_anchor_update(
    resource_id: &[u8],
    function_sig: &[u8],
    nonce: u32,
    src_chain_id: u32,
    latest_leaf_index: u32,
    merkle_root: &[u8],
) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(resource_id);
    data.extend_from_slice(function_sig);
    data.extend_from_slice(&nonce.to_be_bytes());
    data.extend_from_slice(&src_chain_id.to_be_bytes());
    data.extend_from_slice(&latest_leaf_index.to_be_bytes());
    data.extend_from_slice(merkle_root);
    return data;
}