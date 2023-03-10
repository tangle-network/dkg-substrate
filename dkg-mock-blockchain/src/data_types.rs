use crate::{mock_blockchain_config::ErrorCase, FinalityNotification, ImportNotification};
use serde::{Deserialize, Serialize};
use codec::WrapperTypeDecode;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum MockBlockChainEvent<B: crate::BlockTraitForTest> {
	FinalityNotification { notification: FinalityNotification<B>, command: AttachedCommandMetadata },
	ImportNotification { notification: ImportNotification<B>, command: AttachedCommandMetadata },
	TestCase { trace_id: Uuid, test: TestCase },
}

pub trait BlockTraitForTest: sp_runtime::traits::Block+ Unpin {

}
impl<T: sp_runtime::traits::Block + Unpin> BlockTraitForTest for T 
	where <Self as sp_runtime::traits::Block>::Hash: WrapperTypeDecode {

}

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
	error: Option<String>,
	result: Option<String>,
	trace_id: Uuid,
}

/// For keeping track of various events sent to subscribing clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestCase {
	Valid,
	Invalid(ErrorCase),
}

mod serde_impl {
	use crate::{AttachedCommandMetadata, FinalityNotification, ImportNotification, TestCase};
	use codec::{Decode, Encode};
	use serde::{Deserialize, Serialize};
	use uuid::Uuid;

	use crate::MockBlockChainEvent;

	// Serialize/Deserialize is not implemented for FinalityNotification and ImportNotification
	// However, codec is implemented for all their inner field
	#[derive(Serialize, Deserialize)]
	enum IntermediateMockBlockChainEvent {
		FinalityNotification {
			notification: IntermediateFinalityNotification,
			command: AttachedCommandMetadata,
		},
		ImportNotification {
			notification: IntermediateImportNotification,
			command: AttachedCommandMetadata,
		},
		TestCase {
			trace_id: Uuid,
			test: TestCase,
		},
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
		// Stale branches heads.
		pub stale_heads: Vec<u8>,
	}

	#[derive(Serialize, Deserialize)]
	struct IntermediateImportNotification {
		// Imported block header hash.
		hash: Vec<u8>,
		// Imported block origin.
		origin: Vec<u8>,
		// Imported block header.
		header: Vec<u8>,
		// Is this the new best block.
		is_new_best: bool,
		// Tree route from old best to new best parent.
		// (will be disabled for testing purposed)
		tree_route: Option<()>,
	}

	impl<B: crate::BlockTraitForTest> Serialize for MockBlockChainEvent<B> {
		fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where
			S: serde::Serializer,
		{
			match self {
				MockBlockChainEvent::TestCase { trace_id, test } => {
					let intermediate = IntermediateMockBlockChainEvent::TestCase {
						trace_id: *trace_id,
						test: test.clone(),
					};
					Serialize::serialize(&intermediate, serializer)
				},
				MockBlockChainEvent::FinalityNotification { notification, command } => {
					let intermediate_notification = IntermediateFinalityNotification {
						hash: Encode::encode(&notification.hash),
						tree_route: None,
						header: Encode::encode(&notification.header),
						stale_heads: Encode::encode(&notification.stale_heads),
					};

					let intermediate = IntermediateMockBlockChainEvent::FinalityNotification {
						notification: intermediate_notification,
						command: command.clone(),
					};

					Serialize::serialize(&intermediate, serializer)
				},
				MockBlockChainEvent::ImportNotification { notification, command } => {
					let intermediate_notification = IntermediateImportNotification {
						hash: Encode::encode(&notification.hash),
						origin: Encode::encode(&notification.origin),
						header: Encode::encode(&notification.header),
						is_new_best: notification.is_new_best,
						tree_route: None,
					};

					let intermediate = IntermediateMockBlockChainEvent::ImportNotification {
						notification: intermediate_notification,
						command: command.clone(),
					};

					Serialize::serialize(&intermediate, serializer)
				},
			}
		}
	}

	impl<'de, B: crate::BlockTraitForTest> Deserialize<'de> for MockBlockChainEvent<B> {
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where
			D: serde::Deserializer<'de>,
		{
			let intermediate: IntermediateMockBlockChainEvent =
				Deserialize::deserialize(deserializer)?;
			let event = match intermediate {
				IntermediateMockBlockChainEvent::TestCase { trace_id, test } =>
					MockBlockChainEvent::TestCase { trace_id, test },
				IntermediateMockBlockChainEvent::FinalityNotification { notification, command } => {
					let notification = FinalityNotification::<B> {
						hash: Decode::decode(&mut notification.hash.as_slice()).unwrap(),
						header: Decode::decode(&mut notification.header.as_slice()).unwrap(),
						tree_route: std::sync::Arc::new([]),
						stale_heads: Decode::decode(&mut notification.stale_heads.as_slice())
							.unwrap(),
					};

					MockBlockChainEvent::FinalityNotification { notification, command }
				},
				IntermediateMockBlockChainEvent::ImportNotification { notification, command } => {
					let notification = ImportNotification::<B> {
						hash: Decode::decode(&mut notification.hash.as_slice()).unwrap(),
						header: Decode::decode(&mut notification.header.as_slice()).unwrap(),
						origin: Decode::decode(&mut notification.origin.as_slice()).unwrap(),
						is_new_best: notification.is_new_best,
						tree_route: None,
					};

					MockBlockChainEvent::ImportNotification { notification, command }
				},
			};

			Ok(event)
		}
	}
}
