use frame_support::RuntimeDebug;
use sp_runtime::{create_runtime_str, traits::AtLeast32Bit};
use sp_std::hash::{Hash, Hasher};

use codec::{Decode, Encode};
use sp_std::vec::Vec;

use crate::ChainIdType;

pub const PROPOSAL_SIGNATURE_LENGTH: usize = 65;

pub type ResourceId = [u8; 32];
// Proposal Nonces (4 bytes)
pub type ProposalNonce = u32;

#[derive(Clone, Encode, Decode, RuntimeDebug, scale_info::TypeInfo)]
pub struct RefreshProposal {
	pub nonce: ProposalNonce,
	pub pub_key: Vec<u8>,
}

#[derive(
	Eq, PartialEq, Clone, Encode, Decode, scale_info::TypeInfo, frame_support::RuntimeDebug,
)]
pub struct RefreshProposalSigned {
	pub nonce: ProposalNonce,
	pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, scale_info::TypeInfo)]
pub struct ProposalHeader<ChainId: AtLeast32Bit + Copy + Encode + Decode> {
	pub resource_id: ResourceId,
	pub chain_id: ChainIdType<ChainId>,
	pub function_sig: [u8; 4],
	pub nonce: ProposalNonce,
}

impl<ChainId: AtLeast32Bit + Copy + Encode + Decode> Encode for ProposalHeader<ChainId> {
	fn encode(&self) -> Vec<u8> {
		let mut buf = Vec::new();
		// resource_id contains the chain id already.
		buf.extend_from_slice(&self.resource_id); // 32 bytes
		buf.extend_from_slice(&self.function_sig); // 4 bytes
		buf.extend_from_slice(&self.nonce.to_be_bytes()); // 4 bytes
		buf
	}

	fn encoded_size(&self) -> usize {
		40 // Bytes
	}
}

impl<ChainId: AtLeast32Bit + Copy + Encode + Decode> Decode for ProposalHeader<ChainId> {
	fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
		let mut data = [0u8; 40];
		input.read(&mut data).map_err(|_| {
			codec::Error::from("input bytes are less than the header size (40 bytes)")
		})?;
		// _NOTE_: rustc won't generate bounds check for the following slice
		// since we know the length of the slice is at least 40 bytes already.

		// decode the resourceId is the first 32 bytes
		let mut resource_id = [0u8; 32];
		resource_id.copy_from_slice(&data[0..32]);
		// the chain type is the 5th last byte of the **resourceId**
		let mut chain_type = [0u8; 2];
		chain_type.copy_from_slice(&data[26..28]);
		// the chain id is the last 4 bytes of the **resourceId**
		let mut chain_id_bytes = [0u8; 4];
		chain_id_bytes.copy_from_slice(&resource_id[28..32]);
		let chain_id = u32::from_be_bytes(chain_id_bytes);
		// the function signature is the next first 4 bytes after the resourceId.
		let mut function_sig = [0u8; 4];
		function_sig.copy_from_slice(&data[32..36]);
		// the nonce is the last 4 bytes of the header (also considered as the first arg).
		let mut nonce_bytes = [0u8; 4];
		nonce_bytes.copy_from_slice(&data[36..40]);
		let nonce = u32::from_be_bytes(nonce_bytes);
		let header = ProposalHeader::<ChainId> {
			resource_id,
			chain_id: match chain_type {
				[1, 0] => ChainIdType::EVM(ChainId::from(chain_id)),
				[2, 0] => ChainIdType::Substrate(ChainId::from(chain_id)),
				[3, 1] => ChainIdType::RelayChain(
					create_runtime_str!("polkadot"),
					ChainId::from(chain_id),
				),
				[3, 2] =>
					ChainIdType::RelayChain(create_runtime_str!("kusama"), ChainId::from(chain_id)),
				[4, 0] => ChainIdType::CosmosSDK(ChainId::from(chain_id)),
				[5, 0] => ChainIdType::Solana(ChainId::from(chain_id)),
				_ => return Err(codec::Error::from("invalid chain type")),
			},
			function_sig,
			nonce: ProposalNonce::from(nonce),
		};
		Ok(header)
	}
}

impl<ChainId: AtLeast32Bit + Copy + Encode + Decode> From<ProposalHeader<ChainId>>
	for (ChainIdType<ChainId>, ProposalNonce)
{
	fn from(header: ProposalHeader<ChainId>) -> Self {
		(header.chain_id, header.nonce)
	}
}

impl<ChainId: AtLeast32Bit + Copy + Encode + Decode> From<ProposalHeader<ChainId>>
	for (ResourceId, ChainIdType<ChainId>, ProposalNonce)
{
	fn from(header: ProposalHeader<ChainId>) -> Self {
		(header.resource_id, header.chain_id, header.nonce)
	}
}

#[derive(Debug, Clone, Decode, Encode, Copy, Eq, PartialOrd, Ord, scale_info::TypeInfo)]
pub enum DKGPayloadKey {
	EVMProposal(ProposalNonce),
	RefreshVote(ProposalNonce),
	AnchorUpdateProposal(ProposalNonce),
	TokenAddProposal(ProposalNonce),
	TokenRemoveProposal(ProposalNonce),
	WrappingFeeUpdateProposal(ProposalNonce),
	ResourceIdUpdateProposal(ProposalNonce),
	RescueTokensProposal(ProposalNonce),
	MaxDepositLimitUpdateProposal(ProposalNonce),
	MinWithdrawLimitUpdateProposal(ProposalNonce),
	MaxExtLimitUpdateProposal(ProposalNonce),
	MaxFeeLimitUpdateProposal(ProposalNonce),
}

impl PartialEq for DKGPayloadKey {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::EVMProposal(l0), Self::EVMProposal(r0)) => l0 == r0,
			(Self::RefreshVote(l0), Self::RefreshVote(r0)) => l0 == r0,
			(Self::AnchorUpdateProposal(l0), Self::AnchorUpdateProposal(r0)) => l0 == r0,
			(Self::TokenAddProposal(l0), Self::TokenAddProposal(r0)) => l0 == r0,
			(Self::TokenRemoveProposal(l0), Self::TokenRemoveProposal(r0)) => l0 == r0,
			(Self::WrappingFeeUpdateProposal(l0), Self::WrappingFeeUpdateProposal(r0)) => l0 == r0,
			(Self::ResourceIdUpdateProposal(l0), Self::ResourceIdUpdateProposal(r0)) => l0 == r0,
			(Self::RescueTokensProposal(l0), Self::RescueTokensProposal(r0)) => l0 == r0,
			(Self::MaxDepositLimitUpdateProposal(l0), Self::MaxDepositLimitUpdateProposal(r0)) =>
				l0 == r0,
			(
				Self::MinWithdrawLimitUpdateProposal(l0),
				Self::MinWithdrawLimitUpdateProposal(r0),
			) => l0 == r0,
			(Self::MaxExtLimitUpdateProposal(l0), Self::MaxExtLimitUpdateProposal(r0)) => l0 == r0,
			(Self::MaxFeeLimitUpdateProposal(l0), Self::MaxFeeLimitUpdateProposal(r0)) => l0 == r0,
			_ => false,
		}
	}
}

impl Hash for DKGPayloadKey {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.encode().hash(state)
	}
}
pub enum ProposalAction {
	// sign the proposal with some priority
	Sign(u8),
}

#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub enum Proposal {
	Signed { kind: ProposalKind, data: Vec<u8>, signature: Vec<u8> },
	Unsigned { kind: ProposalKind, data: Vec<u8> },
}

#[derive(Debug, Encode, Decode, Clone, Eq, PartialEq, scale_info::TypeInfo)]
pub enum ProposalKind {
	Refresh,
	EVM,
	AnchorUpdate,
	TokenAdd,
	TokenRemove,
	WrappingFeeUpdate,
	ResourceIdUpdate,
	RescueTokens,
	MaxDepositLimitUpdate,
	MinWithdrawalLimitUpdate,
	MaxExtLimitUpdate,
	MaxFeeLimitUpdate,
}

impl Proposal {
	pub fn data(&self) -> &Vec<u8> {
		match self {
			Proposal::Signed { data, .. } | Proposal::Unsigned { data, .. } => data,
		}
	}

	pub fn signature(&self) -> Vec<u8> {
		match self {
			Proposal::Signed { signature, .. } => signature.clone(),
			Proposal::Unsigned { .. } => Vec::new(),
		}
	}
}

pub trait ProposalHandlerTrait {
	fn handle_unsigned_proposal(
		_proposal: Vec<u8>,
		_action: ProposalAction,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_signed_proposal(
		_prop: Proposal,
		_payload_key: DKGPayloadKey,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_unsigned_refresh_proposal(
		_proposal: RefreshProposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_signed_refresh_proposal(
		_proposal: RefreshProposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_evm_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_anchor_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_token_add_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_token_remove_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_wrapping_fee_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_resource_id_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_rescue_tokens_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_deposit_limit_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_withdraw_limit_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_ext_limit_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}

	fn handle_fee_limit_update_signed_proposal(
		_prop: Proposal,
	) -> frame_support::pallet_prelude::DispatchResult {
		Ok(().into())
	}
}

impl ProposalHandlerTrait for () {}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn proposal_encode_decode() {
		let mut proposal_data = Vec::with_capacity(80);
		let chain_id = ChainIdType::EVM(5002u32);
		let nonce = 0xffu32;
		let resource_id = {
			let mut r = [0u8; 32];
			r[26..28].copy_from_slice(&chain_id.to_type().to_le_bytes());
			r[28..32].copy_from_slice(&chain_id.inner_id().to_be_bytes());
			r
		};
		let function_sig = [0xaa, 0xbb, 0xcc, 0xdd];
		let proposal_header = ProposalHeader { chain_id, function_sig, nonce, resource_id };
		let src_id = 5001u32;
		let last_leaf_index = nonce as u32;
		let merkle_root = [0xeeu8; 32];
		proposal_header.encode_to(&mut proposal_data);
		proposal_data.extend_from_slice(&src_id.to_be_bytes());
		proposal_data.extend_from_slice(&last_leaf_index.to_be_bytes());
		proposal_data.extend_from_slice(&merkle_root);
		assert_eq!(proposal_data.len(), 80);
		let hex = hex::encode(&proposal_data);
		println!("nonce: {}", nonce);
		println!("src_id: {}", src_id);
		println!("r_id: 0x{}", hex::encode(&resource_id));
		println!("Proposal: 0x{}", hex);

		let decoded_head = ProposalHeader::decode(&mut &proposal_data[..]).unwrap();
		assert_eq!(decoded_head, proposal_header);
	}
}
