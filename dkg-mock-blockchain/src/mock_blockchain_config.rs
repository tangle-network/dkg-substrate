use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MockBlockchainConfig {
	// the bind address for the server
	pub bind: String,
	// the minimum number of clients required before starting the MockBlockchain (=n)
	pub min_clients: usize,
	// threshold (=t)
	pub threshold: usize,
	#[serde(default)]
	#[serde(with = "humantime_serde")]
	// the minimum latency in the network. Each client will receive updates at
    // min_simulated_latency + random(0, 0.25*min_simulated_latency)
	pub min_simulated_latency: Option<Duration>,
    // The number of positive cases to run
    pub positive_cases: usize,
	// a set of error cases
	pub error_cases: Vec<ErrorCase>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// A specific error case that the MockBlockchain will attempt to cause in
/// a subscribing client
pub struct ErrorCase {
    pub name: String,
	#[serde(default)]
	#[serde(with = "humantime_serde")]
	// Add an additional amount of delay ontop of the latency before sending the notification to
	// the subscribing client
	pub preprocess_delay: Option<Duration>,
	// The error expected to occur inside one of the subscribing MockClients
	pub expected_error: String,
	// the number of clients to cause the error for. Must be less than or equal to `min_clients`
	pub n_clients: usize,
	// the specific command that each receiving client should cause
	pub command: crate::AttachedCommand,
    // the number of times to run this test case
    pub count: usize
}
