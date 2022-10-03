use super::*;

pub fn parse_evm_log(log_entry_data: Vec<u8>) -> ([u8; 20], [u8; 32], u32) {
	let parsed_log: LogEntry = rlp::decode(log_entry_data.as_slice()).unwrap();

	let address: [u8; 20] = parsed_log.address.0.into();
	let topics = parsed_log.topics;
	let merkle_root: [u8; 32] = topics[1].0.into();
	let nonce = u32::from_be_bytes(convert_nonce_to_fixed_size(topics[2].0.as_bytes()));
	(address, merkle_root, nonce)
}

pub fn convert_nonce_to_fixed_size(nonce: &[u8]) -> [u8; 4] {
	nonce.try_into().unwrap_or_default()
}
