use crate::{crypto::AuthorityId, MisbehaviourType, SessionId, SignerSetId};
use codec::{Decode, Encode};
use sp_std::vec::Vec;

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGKeygenMessage {
	/// Sender id / party index
	pub sender_id: u16,
	/// Serialized keygen msg
	pub keygen_msg: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGOfflineMessage {
	// Identifier
	pub key: Vec<u8>,
	/// Signer set epoch id
	pub signer_set_id: SignerSetId,
	/// Serialized offline stage msg
	pub offline_msg: Vec<u8>,
	// the unsigned proposal this message is associated with
	pub unsigned_proposal_hash: [u8; 32],
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGVoteMessage {
	/// Party index
	pub party_ind: u16,
	/// Key for the vote signature created for
	pub round_key: Vec<u8>,
	/// Serialized partial signature
	pub partial_signature: Vec<u8>,
	// the unsigned proposal this message is associated with
	pub unsigned_proposal_hash: [u8; 32],
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGSignedPayload {
	/// Payload key
	pub key: Vec<u8>,
	/// The payload signatures are collected for.
	pub payload: Vec<u8>,
	/// Runtime compatible signature for the payload
	pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct PublicKeyMessage {
	/// Round ID of DKG protocol
	pub session_id: SessionId,
	/// Public key for the DKG
	pub pub_key: Vec<u8>,
	/// Authority's signature for this public key
	pub signature: Vec<u8>,
}

/// A misbehaviour message for reporting misbehaviour of an authority.
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct MisbehaviourMessage {
	/// Offending type
	pub misbehaviour_type: MisbehaviourType,
	/// Misbehaving round
	pub session_id: SessionId,
	/// Offending authority's id
	pub offender: AuthorityId,
	/// Authority's signature for this report
	pub signature: Vec<u8>,
}

/// A vote message for voting on a new governor for cross-chain applications leveraging the DKG.
/// https://github.com/webb-tools/protocol-solidity/blob/main/packages/contracts/contracts/utils/Governable.sol#L46-L53C3
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct ProposerVoteMessage {
	/// Round ID of DKG protocol
	pub session_id: SessionId,
	/// The leaf index of the proposer
	pub proposer_leaf_index: u32,
	/// The proposed governor
	pub new_governor: Vec<u8>,
	/// The merkle path sibling nodes for the proposer in the proposer set merkle tree
	pub proposer_merkle_path: Vec<[u8; 32]>,
	/// Authority's signature for this vote
	pub signature: Vec<u8>,
}
