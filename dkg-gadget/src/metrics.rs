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

//! DKG Prometheus metrics definition
use prometheus::{register, Counter, Gauge, PrometheusError, Registry, U64};

/// DKG metrics exposed through Prometheus
#[derive(Clone)]
pub(crate) struct Metrics {
	/// Count to total propogated messages
	pub dkg_propagated_messages: Counter<U64>,
	/// Current active validator set id
	pub dkg_validator_set_id: Gauge<U64>,
	/// Total messages received
	pub dkg_inbound_messages: Counter<U64>,
	/// Total error messages received
	pub dkg_error_counter: Counter<U64>,
	/// Current session progress witnessed by dkg worker
	pub dkg_session_progress: Gauge<U64>,
	/// Keygen retry counter for dkg worker
	pub dkg_keygen_retry_counter: Counter<U64>,
	/// The latest block height witnessed by the dkg worker
	pub dkg_latest_block_height: Gauge<U64>,
	/// The signing sets for dkg worker
	pub dkg_signing_sets: Gauge<U64>,
}

impl Metrics {
	pub(crate) fn register(registry: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			dkg_validator_set_id: register(
				Gauge::new("dkg_validator_set_id", "Current DKG active validator set id.")?,
				registry,
			)?,
			dkg_propagated_messages: register(
				Counter::new("dkg_propagated_messages", "Number of DKG messages propagated.")?,
				registry,
			)?,
			dkg_inbound_messages: register(
				Counter::new("dkg_inbound_messages", "Number of DKG messages received.")?,
				registry,
			)?,
			dkg_error_counter: register(
				Counter::new("dkg_error_counter", "Number of DKG errors generated")?,
				registry,
			)?,
			dkg_session_progress: register(
				Gauge::new("dkg_session_progress", "Current DKG session progress")?,
				registry,
			)?,
			dkg_keygen_retry_counter: register(
				Counter::new(
					"dkg_keygen_retry_counter",
					"Number of times Keygen has been retried",
				)?,
				registry,
			)?,
			dkg_latest_block_height: register(
				Gauge::new(
					"dkg_latest_block_height",
					"The blocknumber of highest block seen by dkg worker",
				)?,
				registry,
			)?,
			dkg_signing_sets: register(
				Gauge::new("dkg_signing_sets", "The number of signing sets created")?,
				registry,
			)?,
		})
	}
}

// Note: we use the `format` macro to convert an expr into a `u64`. This will fail,
// if expr does not derive `Display`.
#[macro_export]
macro_rules! metric_set {
	($self:ident, $m:ident, $v:expr) => {{
		let val: u64 = format!("{}", $v).parse().unwrap();

		if let Some(metrics) = $self.metrics.as_ref() {
			metrics.$m.set(val);
		}
	}};
}

#[macro_export]
macro_rules! metric_inc {
	($self:ident, $m:ident) => {{
		if let Some(metrics) = $self.metrics.as_ref() {
			metrics.$m.inc();
		}
	}};
}
