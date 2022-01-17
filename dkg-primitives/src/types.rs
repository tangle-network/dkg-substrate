use codec::{Decode, Encode};
use curv::elliptic::curves::{Point, Scalar, Secp256k1};
use std::fmt;

pub type FE = Scalar<Secp256k1>;
pub type GE = Point<Secp256k1>;

/// A typedef for keygen set id
pub type KeygenSetId = u64;
/// A typedef for signer set id
pub type SignerSetId = u64;
/// A typedef for keygen set id
pub type RoundId = u64;
pub use dkg_runtime_primitives::DKGPayloadKey;

/// DKG (distributed key generation) message.
///
/// A message wrapper intended to be passed between the nodes
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGMessage<AuthorityId> {
	/// Node authority id
	pub id: AuthorityId,
	/// DKG message contents
	pub payload: DKGMsgPayload,
	/// Indentifier for the message
	pub round_id: RoundId,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct SignedDKGMessage<AuthorityId> {
	/// DKG messages
	pub msg: DKGMessage<AuthorityId>,
	/// ECDSA signature of sha3(concatenated message contents)
	pub signature: Option<Vec<u8>>,
}

impl<ID> fmt::Display for DKGMessage<ID> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let label = match self.payload {
			DKGMsgPayload::Keygen(_) => "Keygen",
			DKGMsgPayload::Offline(_) => "Offline",
			DKGMsgPayload::Vote(_) => "Vote",
			DKGMsgPayload::PublicKeyBroadcast(_) => "PublicKeyBroadcast",
		};
		write!(f, "DKGMessage of type {}", label)
	}
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum DKGMsgPayload {
	Keygen(DKGKeygenMessage),
	Offline(DKGOfflineMessage),
	Vote(DKGVoteMessage),
	PublicKeyBroadcast(DKGPublicKeyMessage),
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGKeygenMessage {
	/// Keygen set epoch id
	pub keygen_set_id: KeygenSetId,
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
pub struct DKGPublicKeyMessage {
	pub round_id: RoundId,
	pub pub_key: Vec<u8>,
	/// Authority's signature for this public key
	pub signature: Vec<u8>,
}

impl Stage {
	pub fn get_next(self) -> Stage {
		match self {
			Stage::KeygenReady => Stage::Keygen,
			Stage::Keygen => Stage::OfflineReady,
			Stage::OfflineReady => Stage::OfflineReady,
		}
	}
}

impl MiniStage {
	pub fn get_next(self) -> Self {
		match self {
			_ => Self::ManualReady,
		}
	}
}

#[derive(Debug, Clone)]
pub enum DKGError {
	KeygenMisbehaviour { bad_actors: Vec<u16> },
	KeygenTimeout { bad_actors: Vec<u16> },
	OfflineMisbehaviour { bad_actors: Vec<u16> },
	OfflineTimeout { bad_actors: Vec<u16> },
	SignMisbehaviour { bad_actors: Vec<u16> },
	SignTimeout { bad_actors: Vec<u16> },
	StartKeygen { reason: String },
	CreateOfflineStage { reason: String },
	CriticalError { reason: String },
	GenericError { reason: String }, // TODO: handle other
}
