use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MockBlockchainConfig {
	// the bind address for the server
	bind: String,
	// the minimum number of clients required before starting the MockBlockchain (=n)
	min_clients: usize,
	// threshold (=t)
	threshold: usize,
	#[serde(default)]
	#[serde(with = "humantime_serde")]
	// the minimum latency in the network. Each client will receive updates at
    // base_simulated_latency + random(0, 0.25*base_simulated_latency)
	base_simulated_latency: Option<Duration>,
	// a set of error cases
	error_cases: Vec<ErrorCase>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// A specific error case that the MockBlockchain will attempt to cause in
/// a subscribing client
pub struct ErrorCase {
    name: String,
	#[serde(default)]
	#[serde(with = "humantime_serde")]
	// Add an additional amount of delay ontop of the latency before sending the notification to
	// the subscribing client
	preprocess_delay: Option<Duration>,
	// The error expected to occur inside one of the subscribing MockClients
	expected_error: String,
	// the number of clients to cause the error for. Must be less than or equal to `min_clients`
	n_clients: usize,
	// the specific command that each receiving client should cause
	command: crate::AttachedCommand,
}
