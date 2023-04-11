# DKG Test Orchestrator

## Setting up a config file
The configuration file is defined in rust as:
```rust
pub struct MockBlockchainConfig {
	// the bind address for the server
	pub bind: String,
	// the minimum number of clients required before starting the MockBlockchain (=n)
	pub n_clients: usize,
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
	pub error_cases: Option<Vec<ErrorCase>>,
}
```

The error cases are (currently) defined as:

```rust
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
	pub count: usize,
}
```

### Simple example
The simple example below sets up a singular test case with 10 positive rounds. There will be 3 clients with a threshold of 2.
```toml
# the server bind addr
bind = "127.0.0.1:7777"
n_clients = 3
threshold = 2
min_simulated_latency = "100ms"
positive_cases = 10
```


## Running the orchestrator
```
# Build the dkg-standalone node
cargo build --release -p dkg-standalone-node --features=integration-tests,testing

# run the orchestrator, making sure to use the proper config
cargo run --package dkg-test-orchestrator --features=debug-tracing -- --config /path/to/orchestrator_config.toml --tmp ./tmp
# log files for each client will be individually present inside the ./tmp folder, denoted by their peer IDs
```