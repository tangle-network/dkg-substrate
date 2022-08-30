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

//! DKG gadget specific errors
//!
//! Used for DKG gadget interal error handling only

use std::fmt::Debug;

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{
	keygen,
	keygen::{Error::ProceedRound, ProceedError},
};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
	#[error("Keystore error: {0}")]
	Keystore(String),
	#[error("Signature error: {0}")]
	Signature(String),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MPCError {
	#[error("Party creation error: {0}")]
	CryptoOperation(String),
	#[error("Proceed error : {0}")]
	MPCProceedError(String, Vec<usize>),
}

impl From<keygen::Error> for MPCError {
	fn from(e: keygen::Error) -> Self {
		match e {
			// extract the bad actors from error messages
			ProceedRound(ProceedError::Round2VerifyCommitments(e)) =>
				MPCError::MPCProceedError(e.error_type, e.bad_actors),
			ProceedRound(ProceedError::Round3VerifyVssConstruct(e)) =>
				MPCError::MPCProceedError(e.error_type, e.bad_actors),
			ProceedRound(ProceedError::Round4VerifyDLogProof(e)) =>
				MPCError::MPCProceedError(e.error_type, e.bad_actors),
			_ => MPCError::CryptoOperation(e.to_string()),
		}
	}
}
