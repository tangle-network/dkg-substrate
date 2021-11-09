#[cfg(feature = "std")]
pub mod keys;
#[cfg(feature = "std")]
pub mod rounds;
#[cfg(feature = "std")]
pub mod types;

pub mod traits;

// Engine ID for DKG
pub const DKG_ENGINE_ID: sp_runtime::ConsensusEngineId = *b"DKG_";
