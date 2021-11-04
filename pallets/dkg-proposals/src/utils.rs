use crate::types::ResourceId;

/// Helper function to concatenate a chain ID and some bytes to produce a
/// resource ID. The common format is (31 bytes unique ID + 1 byte chain ID).
pub fn derive_resource_id(chain: u32, id: &[u8]) -> ResourceId {
	let mut r_id: ResourceId = [0; 32];
	let chain = chain.to_le_bytes();
	// last 4 bytes of chain id,
	r_id[28] = chain[0];
	r_id[29] = chain[1];
	r_id[30] = chain[2];
	r_id[31] = chain[3];
	let range = if id.len() > 28 { 28 } else { id.len() }; // Use at most 28 bytes
	for i in 0..range {
		r_id[27 - i] = id[range - 1 - i]; // Ensure left padding for eth compatibility
	}
	return r_id
}
