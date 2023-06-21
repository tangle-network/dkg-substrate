use codec::Encode;
use ethabi::{encode, Token};
use sp_std::{vec, vec::Vec};

use crate::gossip_messages::ProposerVoteMessage;

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
