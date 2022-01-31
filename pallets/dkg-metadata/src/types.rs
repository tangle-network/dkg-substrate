use crate::*;
use codec::{Decode, Encode, MaxEncodedLen};
#[derive(Default, Encode, Decode, Clone, PartialEq, Eq, scale_info::TypeInfo)]
pub struct RoundMetadata {
	pub curr_round_pub_key: Vec<u8>,
	pub next_round_pub_key: Vec<u8>,
	pub refresh_signature: Vec<u8>,
}
