use crate::{mock_blockchain_config::ErrorCase, FinalityNotification, ImportNotification};
use codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum MockBlockchainEvent<B: crate::BlockTraitForTest> {
	FinalityNotification { notification: FinalityNotification<B> },
	ImportNotification { notification: ImportNotification<B> },
}

pub trait BlockTraitForTest: sp_runtime::traits::Block + Unpin {}
impl<T: sp_runtime::traits::Block + Unpin> BlockTraitForTest for T {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AttachedCommandMetadata {
	// Specifies the command which the receiving MockClient is expected to take when receiving this
	// message. The message may purposefully attempt to cause an error to try to see how receiving
	// nodes react to the adversity
	command: AttachedCommand,
	// trace ID
	trace_id: Uuid,
}

/// A command given to the receiving MockClient
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AttachedCommand {
	// Tells the client to process the requests as normal
	ProcessAsNormal,
	// Tells the client to not process the request (for this round)
	ErrorDontProcessRequest,
}

/// When a MockClient receives a message, it should attempt to send information
/// about its internal state back to the MockBlockchain server for centralized
/// introspection
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MockClientResponse {
	pub result: Result<(), String>,
	pub trace_id: Uuid,
	pub pub_key: Option<Vec<u8>>,
}

/// For keeping track of various events sent to subscribing clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestCase {
	Valid,
	Invalid(ErrorCase),
}

mod serde_impl {
	use crate::{FinalityNotification, ImportNotification};
	use codec::{Decode, Encode};
	use sc_client_api::FinalizeSummary;
	use serde::{Deserialize, Serialize};
	use sp_consensus::BlockOrigin;

	use crate::MockBlockchainEvent;

	// Serialize/Deserialize is not implemented for FinalityNotification and ImportNotification
	// However, codec is implemented for all their inner field
	#[derive(Serialize, Deserialize)]
	enum IntermediateMockBlockchainEvent {
		FinalityNotification { notification: IntermediateFinalityNotification },
		ImportNotification { notification: IntermediateImportNotification },
	}

	#[derive(Serialize, Deserialize)]
	struct IntermediateFinalityNotification {
		// Finalized block header hash.
		pub hash: Vec<u8>,
		// Finalized block header.
		pub header: Vec<u8>,
		// Path from the old finalized to new finalized parent (implicitly finalized blocks).
		//
		// This maps to the range `(old_finalized, new_finalized)`.
		pub tree_route: Option<()>,
		// Stale branches heads. (not used)
		pub stale_heads: Option<()>,
	}

	#[derive(Serialize, Deserialize)]
	struct IntermediateImportNotification {
		// Imported block header hash.
		hash: Vec<u8>,
		// Imported block origin. (disabled for testing purposes)
		origin: Option<()>,
		// Imported block header.
		header: Vec<u8>,
		// Is this the new best block.
		is_new_best: bool,
		// Tree route from old best to new best parent.
		// (will be disabled for testing purposed)
		tree_route: Option<()>,
	}

	impl<B: crate::BlockTraitForTest> Serialize for MockBlockchainEvent<B> {
		fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where
			S: serde::Serializer,
		{
			match self {
				MockBlockchainEvent::FinalityNotification { notification } => {
					let intermediate_notification = IntermediateFinalityNotification {
						hash: Encode::encode(&notification.hash),
						tree_route: None,
						header: Encode::encode(&notification.header),
						stale_heads: None,
					};

					let intermediate = IntermediateMockBlockchainEvent::FinalityNotification {
						notification: intermediate_notification,
					};

					Serialize::serialize(&intermediate, serializer)
				},
				MockBlockchainEvent::ImportNotification { notification } => {
					let intermediate_notification = IntermediateImportNotification {
						hash: Encode::encode(&notification.hash),
						origin: None,
						header: Encode::encode(&notification.header),
						is_new_best: notification.is_new_best,
						tree_route: None,
					};

					let intermediate = IntermediateMockBlockchainEvent::ImportNotification {
						notification: intermediate_notification,
					};

					Serialize::serialize(&intermediate, serializer)
				},
			}
		}
	}

	impl<'de, B: crate::BlockTraitForTest> Deserialize<'de> for MockBlockchainEvent<B> {
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where
			D: serde::Deserializer<'de>,
		{
			let intermediate: IntermediateMockBlockchainEvent =
				Deserialize::deserialize(deserializer)?;
			let event = match intermediate {
				IntermediateMockBlockchainEvent::FinalityNotification { notification } => {
					let (tx, _rx) =
						sc_utils::mpsc::tracing_unbounded("mpsc_finality_notification", 999999);
					let summary = FinalizeSummary::<B> {
						header: Decode::decode(&mut notification.header.as_slice()).unwrap(),
						finalized: vec![Decode::decode(&mut notification.hash.as_slice()).unwrap()],
						stale_heads: vec![],
					};
					let notification = FinalityNotification::<B>::from_summary(summary, tx);

					MockBlockchainEvent::FinalityNotification { notification }
				},
				IntermediateMockBlockchainEvent::ImportNotification { notification } => {
					let (tx, _rx) =
						sc_utils::mpsc::tracing_unbounded("mpsc_import_notification", 999999);
					let notification = ImportNotification::<B>::new(
						Decode::decode(&mut notification.hash.as_slice()).unwrap(),
						BlockOrigin::NetworkBroadcast,
						Decode::decode(&mut notification.header.as_slice()).unwrap(),
						notification.is_new_best,
						None,
						tx,
					);

					MockBlockchainEvent::ImportNotification { notification }
				},
			};

			Ok(event)
		}
	}
}

pub type TestBlock = sp_runtime::testing::Block<XtDummy>;
// Xt must impl: 'static + Codec + Sized + Send + Sync + Serialize + Clone + Eq + Debug + Extrinsic
#[derive(Encode, Decode, Serialize, Clone, Eq, PartialEq, Debug)]
pub struct XtDummy;

impl sp_runtime::traits::Extrinsic for XtDummy {
	type Call = ();
	type SignaturePayload = ();
}

pub fn serialize_peer_id<S>(x: &crate::server::PeerId, s: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	Vec::<u8>::serialize(&x.to_bytes(), s)
}

pub fn deserialize_peer_id<'de, D>(data: D) -> Result<crate::server::PeerId, D::Error>
where
	D: serde::Deserializer<'de>,
{
	let bytes: Vec<u8> = Vec::<u8>::deserialize(data)?;
	Ok(crate::server::PeerId::from_bytes(&bytes).unwrap())
}
