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

pub type FE = Scalar<Secp256k1>;
pub type GE = Point<Secp256k1>;

/// A typedef for keygen set id
pub type RoundId = u64;
/// A typedef for signer set id
pub type SignerSetId = u64;

pub use dkg_runtime_primitives::DKGPayloadKey;

/// A Unique identifier.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct Uid([u8; 16]);

impl Uid {
	pub fn random() -> Self {
		let mut uid = [0u8; 16];
		uid.copy_from_slice(&rand::random::<[u8; 16]>());
		Self(uid)
	}

	pub fn from_hash_of<T: Encode>(t: &T) -> Self {
		let mut uid = [0u8; 16];
		uid.copy_from_slice(&sp_core::hashing::blake2_128(&t.encode()));
		Self(uid)
	}
}

impl From<[u8; 16]> for Uid {
	fn from(uid: [u8; 16]) -> Self {
		Self(uid)
	}
}

impl fmt::Display for Uid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// it is 4 parts, each part is 4 bytes, which is just u32.
		// then each number is hex encoded.
		let part1 = u32::from_be_bytes(self.0[0..4].try_into().unwrap());
		let part2 = u32::from_be_bytes(self.0[4..8].try_into().unwrap());
		let part3 = u32::from_be_bytes(self.0[8..12].try_into().unwrap());
		let part4 = u32::from_be_bytes(self.0[12..16].try_into().unwrap());
		write!(f, "{:x}-{:x}-{:x}-{:x}", part1, part2, part3, part4)
	}
}

/// DKGMsgStatus Enum identifies if a message is for an active or queued round
#[derive(Debug, Copy, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub enum DKGMsgStatus {
	/// Active round
	ACTIVE,
	/// Queued round,
	QUEUED,
	/// Unknown
	UNKNOWN,
}

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
		<<B::Header as Header>::Hashing as Hash>::hash_of(&self.msg)
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
			DKGMsgPayload::Keygen(kg) => Some(kg.sender_id as u16),
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
	/// Sender id / party index
	pub sender_id: u16,
	/// Serialized keygen msg
	pub keygen_msg: Vec<u8>,
}

#[derive(Debug, Clone, Decode, Encode)]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct DKGOfflineMessage {
	/// Unique identifier for the offline stage
	/// that this message is intended for.
	pub uid: Uid,
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
	KeygenMisbehaviour { reason: String, bad_actors: Vec<usize> },
	KeygenTimeout { bad_actors: Vec<usize> },
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
			KeygenMisbehaviour { reason, bad_actors } =>
				format!("Keygen misbehaviour: reason : {:?} bad actors: {:?}", reason, bad_actors),
			KeygenTimeout { bad_actors } => format!("Keygen timeout: bad actors: {:?}", bad_actors),
			Vote { reason } => format!("Vote: {}", reason),
			StartKeygen { reason } => format!("Start keygen: {}", reason),
			CreateOfflineStage { reason } => format!("Create offline stage: {}", reason),
			CriticalError { reason } => format!("Critical error: {}", reason),
			GenericError { reason } => format!("Generic error: {}", reason),
			StartOffline { reason } => format!("Unable to start Offline Signing: {}", reason),
			NoAuthorityAccounts => "No Authority accounts found!".to_string(),
			NoHeader => "No Header!".to_string(),
			SignMisbehaviour { reason, bad_actors } =>
				format!("SignMisbehaviour : reason: {:?},  bad actors: {:?}", reason, bad_actors),
		};
		write!(f, "DKGError of type {}", label)
	}
}
