use serde::{Serialize, Deserialize};
use uuid::Uuid;
use async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum MockBlockChainEvent {
    FinalityNotification { notification: FinalityNotification, command: AttachedCommandMetadata },
    ImportNotification { notification: ImportNotification, command: AttachedCommandMetadata },
    StorageChangeNotification { notification: StorageChangeNotification, command: AttachedCommandMetadata }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AttachedCommandMetadata {
    // Specifies the command which the receiving MockClient is expected to take when receiving this
    // message. The message may purposefully attempt to cause an error to try to see how receiving nodes
    // react to the adversity
    command: AttachedCommand,
    // trace ID
    trace_id: Uuid
}

/// A command given to the receiving MockClient
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AttachedCommand {
    ProcessAsNormal,
    ProcessAndExpectError,
}

/// When a MockClient receives a message, it should attempt to send information
/// about its internal state back to the MockBlockchain server for centralized
/// introspection
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MockClientResponse {
    error: Option<String>,
    result: Option<String>,
    trace_id: Uuid
}