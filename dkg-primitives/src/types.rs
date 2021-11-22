use codec::{Decode, Encode};
use std::fmt;

/// A typedef for keygen set id
pub type KeygenSetId = u64;
/// A typedef for signer set id
pub type SignerSetId = u64;
/// A typedef for keygen set id
pub type RoundId = u64;

/// DKG (distributed key generation) message.
///
/// A message wrapper intended to be passed between the nodes
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGMessage<AuthorityId, Key> {
	/// Node authority id
	pub id: AuthorityId,
	/// DKG message contents
	pub payload: DKGMsgPayload<Key>,
	/// Indentifier for the message
	pub round_id: RoundId,
}

impl<ID, K> fmt::Display for DKGMessage<ID, K> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let label = match self.payload {
			DKGMsgPayload::Keygen(_) => "Keygen",
			DKGMsgPayload::Offline(_) => "Offline",
			DKGMsgPayload::Vote(_) => "Vote",
		};
		write!(f, "DKGMessage of type {}", label)
	}
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum DKGMsgPayload<Key> {
	Keygen(DKGKeygenMessage),
	Offline(DKGOfflineMessage),
	Vote(DKGVoteMessage<Key>),
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGKeygenMessage {
	/// Keygen set epoch id
	pub keygen_set_id: KeygenSetId,
	/// Node signature
	pub keygen_msg: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGOfflineMessage {
	/// Signer set epoch id
	pub signer_set_id: SignerSetId,
	/// Node signature
	pub offline_msg: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGVoteMessage<Key> {
	/// Party index
	pub party_ind: u16,
	/// Key for the vote signature created for
	pub round_key: Key,
	/// Node signature
	pub partial_signature: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGSignedPayload<Key, Payload> {
	/// Payload key
	pub key: Key,
	/// The payload signatures are collected for.
	pub payload: Payload,
	/// Signature for the payload
	/// SignatureRecid serialized as Vec<u8>, since SignatureRecid does not support codec
	pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
	KeygenReady,
	Keygen,
	OfflineReady,
	Offline,
	ManualReady,
}

impl Stage {
	pub fn get_next(self) -> Stage {
		match self {
			Stage::KeygenReady => Stage::Keygen,
			Stage::Keygen => Stage::OfflineReady,
			Stage::OfflineReady => Stage::Offline,
			Stage::Offline => Stage::ManualReady,
			Stage::ManualReady => Stage::ManualReady,
		}
	}
}
