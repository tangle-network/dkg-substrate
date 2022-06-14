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
pub trait DKGProposalHandlerApi<BlockHash, Proposal> {
	/// Get the passed DKG proposals that have not been signed by the DKG.
	///
	/// This method calls into a runtime with `DKGProposalHandler` pallet included and
	/// attempts to get the unsigned proposals waiting to be signed.
	/// Optionally, a block hash at which the runtime should be queried can be
	/// specified.
	///
	/// Returns the (full) a Vec<Proposal> of the proposals.
	#[rpc(name = "dkg_proposals_getUnsignedProposals")]
	fn get_unsigned_proposals(&self, at: Option<BlockHash>) -> Result<Vec<Proposal>>;
}

/// A struct that implements the `DKGProposalHandlerApi`.
pub struct DKGProposalHandlerClient<C, M, P> {
	client: Arc<C>,
	_marker: std::marker::PhantomData<(M, P)>,
}

impl<C, M, P> DKGProposalHandlerClient<C, M, P> {
	/// Create new `Merkle` instance with the given reference to the client.
	pub fn new(client: Arc<C>) -> Self {
		Self { client, _marker: Default::default() }
	}
}

impl<C, Block, Proposal> DKGProposalHandlerApi<<Block as BlockT>::Hash, Proposal>
	for DKGProposalHandlerClient<C, Block, Proposal>
where
	Block: BlockT,
	Proposal: Encode + Decode,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + Send + Sync,
	C::Api: DKGProposalHandlerApi<Block, Proposal>,
{
	fn get_unsigned_proposals(&self, at: Option<<Block as BlockT>::Hash>) -> Result<Vec<Element>> {
		let api = self.client.runtime_api();
		let at = BlockId::hash(at.unwrap_or_else(|| self.client.info().best_hash));
		let proposals = api.get_unsigned_proposals(at)?;
		Ok(proposals)
	}
}
