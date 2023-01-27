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
use sp_runtime::traits::{Block, Hash, Header};
use std::fmt;

pub use dkg_runtime_primitives::SessionId;

pub type FE = Scalar<Secp256k1>;
pub type GE = Point<Secp256k1>;

/// A typedef for signer set id
pub type SignerSetId = u64;

pub use dkg_runtime_primitives::DKGPayloadKey;

/// DKGMsgStatus Enum identifies if a message is for an active or queued round
#[derive(Debug, Copy, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum DKGMsgStatus {
	/// Active round
	ACTIVE,
	/// Queued round,
	QUEUED,
}

/// Gossip message struct for all DKG + Webb Protocol messages.
///
/// A message wrapper intended to be passed between the nodes
#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGMessage<AuthorityId> {
	/// Node authority id
	pub sender_id: AuthorityId,
	/// DKG message contents
	pub payload: DKGMsgPayload,
	/// Indentifier for the message
	pub session_id: SessionId,
	/// enum for active or queued
	pub status: DKGMsgStatus,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct SignedDKGMessage<AuthorityId> {
	/// DKG messages
	pub msg: DKGMessage<AuthorityId>,
	/// ECDSA signature of sha3(concatenated message contents)
	pub signature: Option<Vec<u8>>,
}

impl<AuthorityId> SignedDKGMessage<AuthorityId> {
	pub fn message_hash<B: Block>(&self) -> B::Hash
	where
		DKGMessage<AuthorityId>: Encode,
	{
		// in case of Keygen or Offline, we only need to hash the inner raw message bytes that
		// are going to be sent to the state machine.
		let bytes_to_hash = match self.msg.payload {
			DKGMsgPayload::Keygen(ref m) => m.keygen_msg.clone(),
			DKGMsgPayload::Offline(ref m) => m.offline_msg.clone(),
			DKGMsgPayload::Vote(ref m) => m.encode(),
			DKGMsgPayload::PublicKeyBroadcast(ref m) => m.encode(),
			DKGMsgPayload::MisbehaviourBroadcast(ref m) => m.encode(),
		};
		<<B::Header as Header>::Hashing as Hash>::hash_of(&bytes_to_hash)
	}
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
		write!(f, "DKGMessage of type {label}")
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
			DKGMsgPayload::Keygen(kg) => Some(kg.sender_id),
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

	pub fn get_async_index(&self) -> u8 {
		match self {
			DKGMsgPayload::Offline(m) => m.async_index,
			DKGMsgPayload::Vote(m) => m.async_index,
			_ => 0,
		}
	}
}

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
	/// Index in async protocols
	pub async_index: u8,
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
	/// Index in async protocols
	pub async_index: u8,
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
	pub session_id: SessionId,
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
	pub session_id: SessionId,
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
	pub session_id: SessionId,
	pub party_index: u16,
	pub threshold: u16,
	pub parties: u16,
}

pub struct SignParams {
	pub session_id: SessionId,
	pub party_index: u16,
	pub threshold: u16,
	pub parties: u16,
	pub signers: Vec<u16>,
	pub signer_set_id: SignerSetId,
}

#[derive(Debug, Clone)]
pub enum DKGResult {
	Empty,
	KeygenFinished { session_id: SessionId, local_key: Box<LocalKey<Secp256k1>> },
}

#[derive(Debug, Clone)]
pub enum DKGError {
	KeygenMisbehaviour { reason: String, bad_actors: Vec<usize> },
	KeygenTimeout { session_id: SessionId, bad_actors: Vec<usize> },
	StartKeygen { reason: String },
	StartOffline { reason: String },
	CreateOfflineStage { reason: String },
	Vote { reason: String },
	CriticalError { reason: String },
	GenericError { reason: String }, // TODO: handle other
	NoAuthorityAccounts,
	NoHeader,
	SignMisbehaviour { reason: String, bad_actors: Vec<usize> },
}

impl fmt::Display for DKGError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		use DKGError::*;
		let label = match self {
			KeygenMisbehaviour { bad_actors, reason } =>
				format!("Keygen misbehaviour: reason: {reason} bad actors: {bad_actors:?}"),
			KeygenTimeout { bad_actors, session_id } =>
				format!("Keygen timeout @ Session({session_id}): bad actors: {bad_actors:?}"),
			Vote { reason } => format!("Vote: {reason}"),
			StartKeygen { reason } => format!("Start keygen: {reason}"),
			CreateOfflineStage { reason } => format!("Create offline stage: {reason}"),
			CriticalError { reason } => format!("Critical error: {reason}"),
			GenericError { reason } => format!("Generic error: {reason}"),
			StartOffline { reason } => format!("Unable to start Offline Signing: {reason}"),
			NoAuthorityAccounts => "No Authority accounts found!".to_string(),
			NoHeader => "No Header!".to_string(),
			SignMisbehaviour { reason, bad_actors } =>
				format!("SignMisbehaviour: reason: {reason},  bad actors: {bad_actors:?}"),
		};
		write!(f, "DKGError of type {label}")
	}
}
