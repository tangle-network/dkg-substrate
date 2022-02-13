use crate::{
	handlers::{decode_proposals::decode_proposal_header, validate_proposals::ValidationError},
	ChainIdTrait, ChainIdType, DKGPayloadKey, ProposalHeader, ProposalNonce,
};
use codec::{alloc::string::ToString, Decode};
use ethereum::{
	EIP1559TransactionMessage, EIP2930TransactionMessage, LegacyTransactionMessage, TransactionV2,
};
use ethereum_types::Address;

pub struct EvmTxProposal<C: ChainIdTrait> {
	pub chain_id: ChainIdType<C>,
	pub nonce: ProposalNonce,
	pub tx: TransactionV2,
}

/// https://github.com/webb-tools/protocol-solidity/issues/83
/// Proposal Data: [
///     resourceId          - 32 bytes [0..32]
///     functionSig         - 4 bytes  [32..36]
///     nonce               - 4 bytes  [36..40]
///     tokenAddress        - 20 bytes  [40..60]
/// ]
/// Total Bytes: 32 + 4 + 4 + 20 = 60
pub fn create<C: ChainIdTrait>(data: &[u8]) -> Result<EvmTxProposal<C>, ValidationError> {
	if data.len() != 60 {
		return Err(ValidationError::InvalidParameter("Proposal data must be 60 bytes".to_string()))?
	}

	let eth_transaction = TransactionV2::decode(&mut &data[..])
		.map_err(|_| ValidationError::InvalidParameter("Invalid transaction".to_string()))?;

	if !validate_ethereum_tx(&eth_transaction) {
		return Err(ValidationError::InvalidParameter(
			"Ethereum transaction is invalid".to_string(),
		))?
	}

	let (chain_id, nonce) = decode_evm_transaction(&eth_transaction)?;

	// TODO: Add validation over EVM address
	Ok(EvmTxProposal { chain_id, nonce, tx: eth_transaction })
}

fn decode_evm_transaction<C: ChainIdTrait>(
	eth_transaction: &TransactionV2,
) -> core::result::Result<(ChainIdType<C>, ProposalNonce), ValidationError> {
	let (chain_id, nonce) = match eth_transaction {
		TransactionV2::Legacy(tx) => {
			let chain_id: u64 = 0;
			let nonce = tx.nonce.as_u32();
			(chain_id, nonce)
		},
		TransactionV2::EIP2930(tx) => {
			let chain_id: u64 = tx.chain_id;
			let nonce = tx.nonce.as_u32();
			(chain_id, nonce)
		},
		TransactionV2::EIP1559(tx) => {
			let chain_id: u64 = tx.chain_id;
			let nonce = tx.nonce.as_u32();
			(chain_id, nonce)
		},
	};

	let chain_id = match C::try_from(chain_id) {
		Ok(v) => v,
		Err(_) => return Err(ValidationError::InvalidParameter("Invalid chain id".to_string()))?,
	};

	return Ok((ChainIdType::EVM(chain_id), nonce))
}

fn validate_ethereum_tx(eth_transaction: &TransactionV2) -> bool {
	return match eth_transaction {
		TransactionV2::Legacy(_tx) => true,
		TransactionV2::EIP2930(_tx) => true,
		TransactionV2::EIP1559(_tx) => true,
	}
}

#[allow(dead_code)]
fn validate_ethereum_tx_signature(eth_transaction: &TransactionV2) -> bool {
	let (sig_r, sig_s, sig_v, msg_hash) = match eth_transaction {
		TransactionV2::Legacy(tx) => {
			let r = tx.signature.r().clone();
			let s = tx.signature.s().clone();
			let v = tx.signature.standard_v();
			let hash = LegacyTransactionMessage::from(tx.clone()).hash();
			(r, s, v, hash)
		},
		TransactionV2::EIP2930(tx) => {
			let r = tx.r.clone();
			let s = tx.s.clone();
			let v = if tx.odd_y_parity { 1 } else { 0 };
			let hash = EIP2930TransactionMessage::from(tx.clone()).hash();
			(r, s, v, hash)
		},
		TransactionV2::EIP1559(tx) => {
			let r = tx.r.clone();
			let s = tx.s.clone();
			let v = if tx.odd_y_parity { 1 } else { 0 };
			let hash = EIP1559TransactionMessage::from(tx.clone()).hash();
			(r, s, v, hash)
		},
	};

	let mut sig = [0u8; 65];
	sig[0..32].copy_from_slice(&sig_r[..]);
	sig[32..64].copy_from_slice(&sig_s[..]);
	sig[64] = sig_v;
	let mut msg = [0u8; 32];
	msg.copy_from_slice(&msg_hash[..]);

	return sp_io::crypto::secp256k1_ecdsa_recover(&sig, &msg).is_ok()
}
