pub(crate) mod data_types;
pub(crate) mod mock_blockchain_config;
pub(crate) mod server;
pub mod transport;

pub use data_types::*;


// types needed in order to implement the Client type
pub type FinalityNotification<B> = sc_client_api::client::FinalityNotification<B>;
pub type ImportNotification<B> = sc_client_api::client::BlockImportNotification<B>;
pub type StorageChangeNotification<B> = sc_client_api::notifications::StorageNotification<B>;