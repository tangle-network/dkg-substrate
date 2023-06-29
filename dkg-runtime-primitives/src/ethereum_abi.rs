use codec::{Decode, Encode};
use ethabi::{encode, Token};
use sp_std::{vec, vec::Vec};

#[allow(clippy::wrong_self_convention)]
pub trait IntoAbiToken {
	fn into_abi(&self) -> Token;
	fn encode_abi(&self) -> Vec<u8> {
		let token = self.into_abi();

		encode(&[token])
	}
}

impl IntoAbiToken for [u8; 32] {
	fn into_abi(&self) -> Token {
		Token::Bytes(self.to_vec())
	}
}

/// A vote message for voting on a new governor for cross-chain applications leveraging the DKG.
// https://github.com/webb-tools/protocol-solidity/blob/main/packages/contracts/contracts/utils/Governable.sol#L10-L17
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct ProposerVoteMessage {
	pub proposer_leaf_index: u32,
	/// The proposed governor
	pub new_governor: Vec<u8>,
	/// The merkle path sibling nodes for the proposer in the proposer set merkle tree
	pub proposer_merkle_path: Vec<[u8; 32]>,
}

impl IntoAbiToken for ProposerVoteMessage {
	fn into_abi(&self) -> Token {
		let tokens = vec![
			Token::Bytes(self.proposer_leaf_index.encode()),
			Token::Bytes(self.new_governor.encode()),
		];
		let merkle_path_tokens =
			self.proposer_merkle_path.iter().map(|x| x.into_abi()).collect::<Vec<Token>>();
		Token::Tuple([tokens, merkle_path_tokens].concat())
	}
}
