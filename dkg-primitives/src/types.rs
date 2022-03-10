use crate::rounds::LocalKey;
use codec::{Decode, Encode};
use curv::elliptic::curves::{Point, Scalar, Secp256k1};
use dkg_runtime_primitives::crypto::AuthorityId;
use std::fmt;

pub type FE = Scalar<Secp256k1>;
pub type GE = Point<Secp256k1>;

/// A typedef for keygen set id
pub type RoundId = u64;
/// A typedef for signer set id
pub type SignerSetId = u64;

pub use dkg_runtime_primitives::DKGPayloadKey;

/// Gossip message struct for all DKG + Webb Protocol messages.
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
			DKGMsgPayload::MisbehaviourBroadcast(_) => "MisbehaviourBroadcast",
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
	MisbehaviourBroadcast(DKGMisbehaviourMessage),
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGKeygenMessage {
	/// Keygen set epoch id
	pub round_id: RoundId,
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

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGMisbehaviourMessage {
	pub round_id: RoundId,
	/// Offending authority's id
	pub offender: AuthorityId,
	/// Authority's signature for this report
	pub signature: Vec<u8>,
}

pub const KEYGEN_TIMEOUT: u32 = 10;
pub const OFFLINE_TIMEOUT: u32 = 10;
pub const SIGN_TIMEOUT: u32 = 3;

pub trait DKGRoundsSM<Payload, Output, Clock> {
	fn proceed(&mut self, _at: Clock) -> Result<bool, DKGError> {
		Ok(false)
	}

	fn get_outgoing(&mut self) -> Vec<Payload> {
		vec![]
	}

	fn handle_incoming(&mut self, _data: Payload, _at: Clock) -> Result<(), DKGError> {
		Ok(())
	}

	fn is_finished(&self) -> bool {
		false
	}

	fn try_finish(self) -> Result<Output, DKGError>;
}

pub struct KeygenParams {
	pub round_id: RoundId,
	pub party_index: u16,
	pub threshold: u16,
	pub parties: u16,
}

pub struct SignParams {
	pub round_id: RoundId,
	pub party_index: u16,
	pub threshold: u16,
	pub parties: u16,
	pub signers: Vec<u16>,
	pub signer_set_id: SignerSetId,
}

#[derive(Debug, Clone)]
pub enum DKGResult {
	Empty,
	KeygenFinished { round_id: RoundId, local_key: LocalKey<Secp256k1> },
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
	Vote { reason: String },
	CriticalError { reason: String },
	GenericError { reason: String }, // TODO: handle other
	SMNotFinished,
	NotListeningForPublicKey,
	NoAuthorityAccounts,
	NoHeader,
}

impl DKGError {
	pub fn to_string(&self) -> String {
		match self {
			DKGError::KeygenMisbehaviour { bad_actors } =>
				format!("Keygen misbehaviour: bad actors: {:?}", bad_actors),
			DKGError::KeygenTimeout { bad_actors } =>
				format!("Keygen timeout: bad actors: {:?}", bad_actors),
			DKGError::OfflineMisbehaviour { bad_actors } =>
				format!("Offline misbehaviour: bad actors: {:?}", bad_actors),
			DKGError::OfflineTimeout { bad_actors } =>
				format!("Offline timeout: bad actors: {:?}", bad_actors),
			DKGError::SignMisbehaviour { bad_actors } =>
				format!("Sign misbehaviour: bad actors: {:?}", bad_actors),
			DKGError::SignTimeout { bad_actors } =>
				format!("Sign timeout: bad actors: {:?}", bad_actors),
			DKGError::Vote { reason } => format!("Vote: {}", reason),
			DKGError::StartKeygen { reason } => format!("Start keygen: {}", reason),
			DKGError::CreateOfflineStage { reason } => format!("Create offline stage: {}", reason),
			DKGError::CriticalError { reason } => format!("Critical error: {}", reason),
			DKGError::GenericError { reason } => format!("Generic error: {}", reason),
			DKGError::SMNotFinished => format!("SM not finished"),
			_ => format!("Unknown error"),
		}
	}
}
