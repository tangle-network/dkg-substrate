// This file is part of Webb.

// Copyright (C) 2021 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DKG Prometheus metrics definition

use prometheus::{register, Counter, Gauge, PrometheusError, Registry, U64};

/// DKG metrics exposed through Prometheus
pub(crate) struct Metrics {
	/// Current active validator set id
	pub dkg_validator_set_id: Gauge<U64>,
	/// Total number of votes sent by this node
	pub dkg_votes_sent: Gauge<U64>,
	/// Most recent concluded voting round
	pub dkg_round_concluded: Gauge<U64>,
	/// Best block finalized by DKG
	pub dkg_best_block: Gauge<U64>,
	/// Next block DKG should vote on
	pub dkg_should_vote_on: Gauge<U64>,
	/// Number of sessions without a signed commitment
	pub dkg_skipped_sessions: Counter<U64>,
}

impl Metrics {
	pub(crate) fn register(registry: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			dkg_validator_set_id: register(
				Gauge::new("dkg_validator_set_id", "Current DKG active validator set id.")?,
				registry,
			)?,
			dkg_votes_sent: register(
				Gauge::new("dkg_votes_sent", "Number of votes sent by this node")?,
				registry,
			)?,
			dkg_round_concluded: register(
				Gauge::new("dkg_round_concluded", "Voting round, that has been concluded")?,
				registry,
			)?,
			dkg_best_block: register(
				Gauge::new("dkg_best_block", "Best block finalized by DKG")?,
				registry,
			)?,
			dkg_should_vote_on: register(
				Gauge::new("dkg_should_vote_on", "Next block, DKG should vote on")?,
				registry,
			)?,
			dkg_skipped_sessions: register(
				Counter::new(
					"dkg_skipped_sessions",
					"Number of sessions without a signed commitment",
				)?,
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
