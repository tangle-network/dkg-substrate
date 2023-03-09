use crate::{
	mock_blockchain_config::ErrorCase, FinalityNotification, ImportNotification,
	StorageChangeNotification,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(bound = "")]
pub enum MockBlockChainEvent<B: sp_runtime::traits::Block> {
	FinalityNotification {
		#[serde(bound = "")]
		notification: FinalityNotification<B>,
		command: AttachedCommandMetadata,
	},
	ImportNotification {
		#[serde(bound = "")]
		notification: ImportNotification<B>,
		command: AttachedCommandMetadata,
	},
	StorageChangeNotification {
		#[serde(bound = "")]
		notification: StorageChangeNotification<B>,
		command: AttachedCommandMetadata,
	},
	TestCase {
		trace_id: Uuid,
		test: TestCase
	}
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
