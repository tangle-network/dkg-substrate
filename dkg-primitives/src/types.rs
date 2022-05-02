// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use codec::{Decode, Encode};
use curv::elliptic::curves::{Point, Scalar, Secp256k1};
use dkg_runtime_primitives::{crypto::AuthorityId, MisbehaviourType};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::LocalKey;
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

impl DKGMsgPayload {
	/// NOTE: this is hacky
	/// TODO: Change enums for keygen, offline, vote
	pub fn async_proto_only_get_sender_id(&self) -> Option<u16> {
		match self {
			DKGMsgPayload::Keygen(kg) => Some(kg.round_id as u16),
			DKGMsgPayload::Offline(offline) => Some(offline.signer_set_id as u16),
			DKGMsgPayload::Vote(vote) => Some(vote.party_ind),
			_ => None,
		}
	}

	pub fn get_type(&self) -> &'static str {
		match self {
			DKGMsgPayload::Keygen(_) => "keygen",
			DKGMsgPayload::Offline(_) => "offline",
			DKGMsgPayload::Vote(_) => "vote",
			DKGMsgPayload::PublicKeyBroadcast(_) => "pub_key_broadcast",
			DKGMsgPayload::MisbehaviourBroadcast(_) => "misbehaviour",
		}
	}
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
	/// Round ID of DKG protocol
	pub round_id: RoundId,
	/// Public key for the DKG
	pub pub_key: Vec<u8>,
	/// Authority's signature for this public key
	pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGMisbehaviourMessage {
	/// Offending type
	pub misbehaviour_type: MisbehaviourType,
	/// Misbehaving round
	pub round_id: RoundId,
	/// Offending authority's id
	pub offender: AuthorityId,
	/// Authority's signature for this report
	pub signature: Vec<u8>,
}

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
	KeygenFinished { round_id: RoundId, local_key: Box<LocalKey<Secp256k1>> },
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

impl fmt::Display for DKGError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let label = match self {
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
			DKGError::SMNotFinished => "SM not finished".to_string(),
			_ => "Unknown error".to_string(),
		};
		write!(f, "DKGError of type {}", label)
	}
}
