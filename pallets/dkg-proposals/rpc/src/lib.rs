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
//
#![allow(clippy::unnecessary_mut_passed)]

use std::sync::Arc;

use jsonrpc_core::{Error, ErrorCode, Result};
use jsonrpc_derive::rpc;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{generic::BlockId, traits::Block as BlockT};

/// Merkle RPC methods.
#[rpc]
pub trait DKGProposalsApi<BlockHash, Proposal, ProposalVotes> {
	/// Get the DKG proposals that are active for voting.
	///
	/// This method calls into a runtime with `DKGProposals` pallet included and
	/// attempts to get the pending proposals open for voting.
	/// Optionally, a block hash at which the runtime should be queried can be
	/// specified.
	///
	/// Returns the (full) a Vec<Proposal> of the proposals.
	#[rpc(name = "dkg_proposals_getPendingProposals")]
	fn get_pending_proposals(&self, at: Option<BlockHash>) -> Result<Vec<Proposal>>;

	/// Get the DKG proposal votes for a given active proposal.
	///
	/// This method calls into a runtime with `DKGProposals` pallet included and
	/// attempts to get the voting data for a given proposal.
	/// Optionally, a block hash at which the runtime should be queried can be
	/// specified.
	///
	/// Returns the (full) ProposalVotes of the given proposal.
	#[rpc(name = "dkg_proposals_getPendingProposals")]
	fn get_pending_proposal_votes(
		&self,
		proposal_id: u32,
		at: Option<BlockHash>,
	) -> Result<ProposalVotes>;
}

/// A struct that implements the `DKGProposalsApi`.
pub struct DKGProposalsClient<C, M, P, PV> {
	client: Arc<C>,
	_marker: std::marker::PhantomData<(M, P, PV)>,
}

impl<C, M, P, PV> DKGProposalsClient<C, M, P, PV> {
	/// Create new `Merkle` instance with the given reference to the client.
	pub fn new(client: Arc<C>) -> Self {
		Self { client, _marker: Default::default() }
	}
}

impl<C, Block, Proposal, ProposalVotes>
	DKGProposalsApi<<Block as BlockT>::Hash, Proposal, ProposalVotes>
	for DKGProposalsClient<C, Block, Proposal, ProposalVotes>
where
	Block: BlockT,
	Proposal: Encode + Decode,
	ProposalVotes: Encode + Decode,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + Send + Sync + 'static,
	C::Api: DKGProposalsApi<Block, Proposal, ProposalVotes>,
{
	fn get_pending_proposals(&self, at: Option<<Block as BlockT>::Hash>) -> Result<Vec<Proposal>> {
		let api = self.client.runtime_api();
		let at = BlockId::hash(at.unwrap_or_else(|| self.client.info().best_hash));
		let proposals = api.get_pending_proposals(at)?;
		Ok(proposals)
	}

	fn get_pending_proposal_votes(
		&self,
		proposal_id: u32,
		at: Option<<Block as BlockT>::Hash>,
	) -> Result<ProposalVotes> {
		let api = self.client.runtime_api();
		let at = BlockId::hash(at.unwrap_or_else(|| self.client.info().best_hash));
		let proposal_votes = api.get_pending_proposal_votes(proposal_id, at)?;
		Ok(proposal_votes)
	}
}
