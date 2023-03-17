use std::marker::PhantomData;

use sc_client_api::{BlockchainEvents, HeaderBackend, AuxStore};
use sp_api::AsTrieBackend;
use sp_api::{BlockT, ProvideRuntimeApi, ApiExt, offchain::storage::InMemOffchainStorage};
use tokio::net::ToSocketAddrs;
use dkg_mock_blockchain::TestBlock;
use futures::{StreamExt, SinkExt};
use dkg_mock_blockchain::transport::ProtocolPacket;
use sp_api::StateBackend;
use sp_runtime::traits::BlakeTwo256;
use parking_lot::RwLock;
use sp_state_machine::*;
use sp_state_machine::backend::Consolidate;
use hash_db::HashDB;
use sp_trie::HashDBT;
use dkg_runtime_primitives::crypto::AuthorityId;

use crate::{worker::DKGWorker, gossip_engine::GossipEngineIface};

/// When peers use a Client, the streams they receive are suppose
/// to come from the BlockChain. However, for testing purposes, we will mock
/// the blockchain and thus send pseudo notifications to subscribing clients.
/// To allow flexibility in the design, each [`MockClient`] will need to connect
/// to a MockBlockchain service that periodically sends notifications to all connected
/// clients.
pub struct MockClient<GE> {
	_pd: PhantomData<GE>
}

#[derive(Clone)]
pub struct TestBackend {

}

impl<GE: GossipEngineIface> MockClient<GE> {
	pub(crate) async fn connect<T: ToSocketAddrs>(mock_bc_addr: T, peer_id: Vec<u8>, dkg_worker: DKGWorker<TestBlock, TestBackend, TestBackend, GE>) -> std::io::Result<Self> {
		let socket = tokio::net::TcpStream::connect(mock_bc_addr).await?;
		let task = async move {
			let (mut tx, mut rx) = dkg_mock_blockchain::transport::bind_transport::<TestBlock>(socket);

			while let Some(packet) = rx.next().await {
				match packet {
					ProtocolPacket::InitialHandshake => {
						// pong back the handshake response
						tx.send(ProtocolPacket::InitialHandshakeResponse { peer_id: peer_id.clone() }).await.unwrap();
					}
					ProtocolPacket::BlockChainToClient { event } => {
						todo!()
					}
					ProtocolPacket::Halt => {
						dkg_logging::info!(target: "dkg", "Received HALT command from the orchestrator");
						std::process::exit(0);
					}

					packet => {
						panic!("Received unexpected packet: {packet:?}")
					}
    			}
			}

			panic!("The connection to the MockBlockchain died")
		};

		Ok(MockClient { _pd: Default::default() })
	}
}

impl BlockchainEvents<TestBlock> for TestBackend {
	fn finality_notification_stream(&self) -> sc_client_api::FinalityNotifications<TestBlock> {
		todo!()
	}

	fn import_notification_stream(&self) -> sc_client_api::ImportNotifications<TestBlock> {
		todo!()
	}

	fn storage_changes_notification_stream(
		&self,
		filter_keys: Option<&[sc_client_api::StorageKey]>,
		child_filter_keys: Option<
			&[(sc_client_api::StorageKey, Option<Vec<sc_client_api::StorageKey>>)],
		>,
	) -> sp_blockchain::Result<sc_client_api::StorageEventStream<<TestBlock as BlockT>::Hash>> {
		todo!()
	}
}


impl sc_client_api::Backend<TestBlock> for TestBackend {
    type BlockImportOperation = DummyStateBackend;
    type Blockchain = sc_client_api::in_mem::Blockchain<TestBlock>;
    type State = DummyStateBackend;
    type OffchainStorage = InMemOffchainStorage;

    fn begin_operation(&self) -> sp_blockchain::Result<Self::BlockImportOperation> {
        todo!()
    }

    fn begin_state_operation(
		&self,
		operation: &mut Self::BlockImportOperation,
		block: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn commit_operation(
		&self,
		transaction: Self::BlockImportOperation,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn finalize_block(
		&self,
		hash: <TestBlock as BlockT>::Hash,
		justification: Option<sp_runtime::Justification>,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn append_justification(
		&self,
		hash: <TestBlock as BlockT>::Hash,
		justification: sp_runtime::Justification,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn blockchain(&self) -> &Self::Blockchain {
        todo!()
    }

    fn usage_info(&self) -> Option<sc_client_api::UsageInfo> {
        todo!()
    }

    fn offchain_storage(&self) -> Option<Self::OffchainStorage> {
        todo!()
    }

    fn state_at(&self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<Self::State> {
        todo!()
    }

    fn revert(
		&self,
		n: sp_api::NumberFor<TestBlock>,
		revert_finalized: bool,
	) -> sp_blockchain::Result<(sp_api::NumberFor<TestBlock>, std::collections::HashSet<<TestBlock as BlockT>::Hash>)> {
        todo!()
    }

    fn remove_leaf_block(&self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn get_import_lock(&self) -> &RwLock<()> {
        todo!()
    }

    fn requires_full_sync(&self) -> bool {
        todo!()
    }
}


impl HeaderBackend<TestBlock> for TestBackend {
    fn header(&self, id: sp_api::BlockId<TestBlock>) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Header>> {
        todo!()
    }

    fn info(&self) -> sp_blockchain::Info<TestBlock> {
        todo!()
    }

    fn status(&self, id: sp_api::BlockId<TestBlock>) -> sp_blockchain::Result<sp_blockchain::BlockStatus> {
        todo!()
    }

    fn number(
		&self,
		hash: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<Option<<<TestBlock as BlockT>::Header as sp_api::HeaderT>::Number>> {
        todo!()
    }

    fn hash(&self, number: sp_api::NumberFor<TestBlock>) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Hash>> {
        todo!()
    }
}


impl ProvideRuntimeApi<TestBlock> for TestBackend {
	type Api = DummyApi;
	fn runtime_api(&self) -> sp_api::ApiRef<Self::Api> {
		sp_api::ApiRef::from(DummyApi)
	}
}
impl AuxStore for TestBackend {
    fn insert_aux<
		'a,
		'b: 'a,
		'c: 'a,
		I: IntoIterator<Item = &'a (&'c [u8], &'c [u8])>,
		D: IntoIterator<Item = &'a &'b [u8]>,
	>(
		&self,
		insert: I,
		delete: D,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn get_aux(&self, key: &[u8]) -> sp_blockchain::Result<Option<Vec<u8>>> {
        todo!()
    }
}

pub struct DummyApi;

#[derive(Debug)]
pub struct DummyStateBackend;
#[derive(Debug)]
pub struct DummyError(String);

impl std::fmt::Display for DummyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StateBackend<BlakeTwo256> for DummyStateBackend {
    type Error = DummyError;

    type Transaction = DummyOverlay;

    type TrieBackendStorage = DummyStateBackend;

    fn storage(&self, key: &[u8]) -> Result<Option<StorageValue>, Self::Error> {
        todo!()
    }

    fn storage_hash(&self, key: &[u8]) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
        todo!()
    }

    fn child_storage(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<StorageValue>, Self::Error> {
        todo!()
    }

    fn child_storage_hash(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
        todo!()
    }

    fn next_storage_key(&self, key: &[u8]) -> Result<Option<StorageKey>, Self::Error> {
        todo!()
    }

    fn next_child_storage_key(
		&self,
		child_info: &sc_client_api::ChildInfo,
		key: &[u8],
	) -> Result<Option<StorageKey>, Self::Error> {
        todo!()
    }

    fn apply_to_key_values_while<F: FnMut(Vec<u8>, Vec<u8>) -> bool>(
		&self,
		child_info: Option<&sc_client_api::ChildInfo>,
		prefix: Option<&[u8]>,
		start_at: Option<&[u8]>,
		f: F,
		allow_missing: bool,
	) -> Result<bool, Self::Error> {
        todo!()
    }

    fn apply_to_keys_while<F: FnMut(&[u8]) -> bool>(
		&self,
		child_info: Option<&sc_client_api::ChildInfo>,
		prefix: Option<&[u8]>,
		start_at: Option<&[u8]>,
		f: F,
	) {
        todo!()
    }

    fn for_key_values_with_prefix<F: FnMut(&[u8], &[u8])>(&self, prefix: &[u8], f: F) {
        todo!()
    }

    fn for_child_keys_with_prefix<F: FnMut(&[u8])>(
		&self,
		child_info: &sc_client_api::ChildInfo,
		prefix: &[u8],
		f: F,
	) {
        todo!()
    }

    fn storage_root<'a>(
		&self,
		delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, Self::Transaction)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord {
        todo!()
    }

    fn child_storage_root<'a>(
		&self,
		child_info: &sc_client_api::ChildInfo,
		delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, bool, Self::Transaction)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord {
        todo!()
    }

    fn pairs(&self) -> Vec<(StorageKey, StorageValue)> {
        todo!()
    }

    fn register_overlay_stats(&self, _stats: &StateMachineStats) {
        todo!()
    }

    fn usage_info(&self) -> UsageInfo {
        todo!()
    }
}

impl ApiExt<TestBlock> for DummyApi {
    type StateBackend = DummyStateBackend;

    fn execute_in_transaction<F: FnOnce(&Self) -> sp_api::TransactionOutcome<R>, R>(&self, call: F) -> R
	where
		Self: Sized {
        todo!()
    }

    fn has_api<A: sp_api::RuntimeApiInfo + ?Sized>(&self, at: &sp_api::BlockId<TestBlock>) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn has_api_with<A: sp_api::RuntimeApiInfo + ?Sized, P: Fn(u32) -> bool>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
		pred: P,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn api_version<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
	) -> Result<Option<u32>, sp_api::ApiError>
	where
		Self: Sized {
        todo!()
    }

    fn record_proof(&mut self) {
        todo!()
    }

    fn extract_proof(&mut self) -> Option<sc_client_api::StorageProof> {
        todo!()
    }

    fn proof_recorder(&self) -> Option<sp_api::ProofRecorder<TestBlock>> {
        todo!()
    }

    fn into_storage_changes(
		&self,
		backend: &Self::StateBackend,
		parent_hash: <TestBlock as BlockT>::Hash,
	) -> Result<sp_api::StorageChanges<Self::StateBackend, TestBlock>, String>
	where
		Self: Sized {
        todo!()
    }
}

impl AsTrieBackend<BlakeTwo256, Vec<u8>> for DummyStateBackend {
    type TrieBackendStorage = Self;

    fn as_trie_backend(&self) -> &sp_api::TrieBackend<Self::TrieBackendStorage, BlakeTwo256, Vec<u8>> {
        todo!()
    }
}

#[derive(Default)]
pub struct DummyOverlay;
impl HashDBT<BlakeTwo256, Vec<u8>> for DummyOverlay {
	fn get(&self, _: &<BlakeTwo256 as sp_core::Hasher>::Out, _: (&[u8], std::option::Option<u8>)) -> std::option::Option<Vec<u8>> { todo!() }
	fn contains(&self, _: &<BlakeTwo256 as sp_core::Hasher>::Out, _: (&[u8], std::option::Option<u8>)) -> bool { todo!() }
	fn insert(&mut self, _: (&[u8], std::option::Option<u8>), _: &[u8]) -> <BlakeTwo256 as sp_core::Hasher>::Out { todo!() }
	fn emplace(&mut self, _: <BlakeTwo256 as sp_core::Hasher>::Out, _: (&[u8], std::option::Option<u8>), _: Vec<u8>) { todo!() }
	fn remove(&mut self, _: &<BlakeTwo256 as sp_core::Hasher>::Out, _: (&[u8], std::option::Option<u8>)) { todo!() }
}

impl hash_db::AsHashDB<BlakeTwo256, Vec<u8>> for DummyOverlay {
	fn as_hash_db(&self) -> &dyn HashDB<BlakeTwo256, Vec<u8>> { todo!() }
	fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn HashDB<BlakeTwo256, Vec<u8>> + 'a) { todo!() }
}

impl Consolidate for DummyOverlay {
	fn consolidate(&mut self, _: Self) { todo!() }
}

impl sp_state_machine::TrieBackendStorage<BlakeTwo256> for DummyStateBackend {
	type Overlay = DummyOverlay;
	fn get(&self, _: &<BlakeTwo256 as sp_core::Hasher>::Out, _: (&[u8], std::option::Option<u8>)) -> Result<std::option::Option<Vec<u8>>, std::string::String> { todo!() }
}

pub struct DummyState;

impl sc_client_api::BlockImportOperation<TestBlock> for DummyStateBackend {
    type State = Self;

    fn state(&self) -> sp_blockchain::Result<Option<&Self::State>> {
        todo!()
    }

    fn set_block_data(
		&mut self,
		header: <TestBlock as BlockT>::Header,
		body: Option<Vec<<TestBlock as BlockT>::Extrinsic>>,
		indexed_body: Option<Vec<Vec<u8>>>,
		justifications: Option<sp_runtime::Justifications>,
		state: sc_client_api::NewBlockState,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn update_cache(&mut self, cache: std::collections::HashMap<sp_blockchain::well_known_cache_keys::Id, Vec<u8>>) {
        todo!()
    }

    fn update_db_storage(
		&mut self,
		update: sc_client_api::TransactionForSB<Self::State, TestBlock>,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn set_genesis_state(
		&mut self,
		storage: sp_runtime::Storage,
		commit: bool,
		state_version: sp_api::StateVersion,
	) -> sp_blockchain::Result<<TestBlock as BlockT>::Hash> {
        todo!()
    }

    fn reset_storage(
		&mut self,
		storage: sp_runtime::Storage,
		state_version: sp_api::StateVersion,
	) -> sp_blockchain::Result<<TestBlock as BlockT>::Hash> {
        todo!()
    }

    fn update_storage(
		&mut self,
		update: StorageCollection,
		child_update: ChildStorageCollection,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn insert_aux<I>(&mut self, ops: I) -> sp_blockchain::Result<()>
	where
		I: IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)> {
        todo!()
    }

    fn mark_finalized(
		&mut self,
		hash: <TestBlock as BlockT>::Hash,
		justification: Option<sp_runtime::Justification>,
	) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn mark_head(&mut self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<()> {
        todo!()
    }

    fn update_transaction_index(&mut self, index: Vec<IndexOperation>)
		-> sp_blockchain::Result<()> {
        todo!()
    }
}

impl AsTrieBackend<BlakeTwo256> for DummyStateBackend {
    type TrieBackendStorage = Self;

    fn as_trie_backend(&self) -> &sp_api::TrieBackend<Self::TrieBackendStorage, BlakeTwo256, sp_trie::cache::LocalTrieCache<BlakeTwo256>> {
        todo!()
    }
}

mod dummy_api {
	use super::*;
	use dkg_runtime_primitives::UnsignedProposal;
	use sp_api::*;
	use sp_runtime::Permill;

	frame_support::parameter_types! {
		// How often we trigger a new session.
		pub const Period: BlockNumber = MINUTES;
		pub const Offset: BlockNumber = 0;
	}

	pub type ApiResult<T> = Result<T, sp_api::ApiError>;
	pub type BlockNumber = sp_api::NumberFor<TestBlock>;
	pub type AccountId = sp_runtime::AccountId32;
	pub type Reputation = u128;
	pub type Moment = u64;
	pub const MILLISECS_PER_BLOCK: Moment = 6000;
	pub const SECS_PER_BLOCK: Moment = MILLISECS_PER_BLOCK / 1000;
	pub const MINUTES: BlockNumber = 60 / (SECS_PER_BLOCK as BlockNumber);

	impl sp_api::Core<TestBlock> for DummyApi {
		fn __runtime_api_internal_call_api_at(&self, _: &BlockId<TestBlock>, _: ExecutionContext, _: Vec<u8>, _: &dyn Fn(RuntimeVersion) -> &'static str) -> ApiResult<Vec<u8>> {
			Ok(vec![])
		}
	}

	impl dkg_primitives::DKGApi<TestBlock, AuthorityId, sp_api::NumberFor<TestBlock>> for DummyApi {
		fn __runtime_api_internal_call_api_at(&self, _: &BlockId<TestBlock>, _: ExecutionContext, _: Vec<u8>, _: &dyn Fn(RuntimeVersion) -> &'static str) -> ApiResult<Vec<u8>> {
			// This function for this dummy implementation does nothing
			Ok(vec![])
		}

		fn authority_set(&self, _: &BlockId<TestBlock>) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId>> {
			let authorities = DKG::authorities();
			let authority_set_id = DKG::authority_set_id();
	  
			Ok(dkg_runtime_primitives::AuthoritySet {
			  authorities,
			  id: authority_set_id
			})
		  }
	  
		  fn queued_authority_set(&self, _: &BlockId<TestBlock>) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId>> {
			let queued_authorities = DKG::next_authorities();
			let queued_authority_set_id = DKG::authority_set_id() + 1u64;
	  
			Ok(dkg_runtime_primitives::AuthoritySet {
			  authorities: queued_authorities,
			  id: queued_authority_set_id
			})
		  }
	  
		  fn signature_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			DKG::signature_threshold()
		  }
	  
		  fn keygen_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			DKG::keygen_threshold()
		  }
	  
		  fn next_signature_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			DKG::next_signature_threshold()
		  }
	  
		  fn next_keygen_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			DKG::next_keygen_threshold()
		  }
	  
		  fn should_refresh(&self, _: &BlockId<TestBlock>, block_number: BlockNumber) -> ApiResult<bool> {
			DKG::should_refresh(block_number)
		  }
	  
		  fn next_dkg_pub_key(&self, _: &BlockId<TestBlock>) -> ApiResult<Option<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)>> {
			DKG::next_dkg_public_key()
		  }
	  
		  fn next_pub_key_sig(&self, _: &BlockId<TestBlock>) -> ApiResult<Option<Vec<u8>>> {
			DKG::next_public_key_signature()
		  }
	  
		  fn dkg_pub_key(&self, _: &BlockId<TestBlock>) -> ApiResult<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)> {
			DKG::dkg_public_key()
		  }
	  
		  fn get_best_authorities(&self, _: &BlockId<TestBlock>) -> ApiResult<Vec<(u16, AuthorityId)>> {
			DKG::best_authorities()
		  }
	  
		  fn get_next_best_authorities(&self, _: &BlockId<TestBlock>) -> ApiResult<Vec<(u16, AuthorityId)>> {
			DKG::next_best_authorities()
		  }
	  
		  fn get_current_session_progress(&self, _: &BlockId<TestBlock>, _block_number: BlockNumber) -> ApiResult<Option<Permill>> {
			  todo!()
			  //use frame_support::traits::EstimateNextSessionRotation;
			  //Ok(<pallet_dkg_metadata::DKGPeriodicSessions<Period, Offset, Runtime> as EstimateNextSessionRotation<BlockNumber>>::estimate_current_session_progress(block_number).0)
		  }
	  
		  fn get_unsigned_proposals(&self, _: &BlockId<TestBlock>) -> ApiResult<Vec<UnsignedProposal>> {
			DKGProposalHandler::get_unsigned_proposals()
		  }
	  
		  fn get_max_extrinsic_delay(&self, _: &BlockId<TestBlock>, block_number: BlockNumber) -> ApiResult<BlockNumber> {
			Ok(DKG::max_extrinsic_delay(block_number))
		  }
	  
		  fn get_authority_accounts(&self, _: &BlockId<TestBlock>) -> ApiResult<(Vec<AccountId>, Vec<AccountId>)> {
			Ok((DKG::current_authorities_accounts(), DKG::next_authorities_accounts()))
		  }
	  
		  fn get_reputations(&self, _: &BlockId<TestBlock>, authorities: Vec<AuthorityId>) -> ApiResult<Vec<(AuthorityId, Reputation)>> {
			Ok(authorities.iter().map(|a| (a.clone(), DKG::authority_reputations(a))).collect())
		  }
	  
		  fn get_keygen_jailed(&self, _: &BlockId<TestBlock>, set: Vec<AuthorityId>) -> ApiResult<Vec<AuthorityId>> {
			todo!()
			//Ok(set.iter().filter(|a| pallet_dkg_metadata::JailedKeygenAuthorities::<Runtime>::contains_key(a)).cloned().collect())
		  }
	  
		  fn get_signing_jailed(&self, _: &BlockId<TestBlock>, set: Vec<AuthorityId>) -> ApiResult<Vec<AuthorityId>> {
			todo!()
			//Ok(set.iter().filter(|a| pallet_dkg_metadata::JailedSigningAuthorities::<Runtime>::contains_key(a)).cloned().collect())
		  }
	  
		  fn refresh_nonce(&self, _: &BlockId<TestBlock>) -> ApiResult<u32> {
			DKG::refresh_nonce()
		  }
	  
		  fn should_execute_new_keygen(&self, _: &BlockId<TestBlock>) -> ApiResult<bool> {
			  DKG::should_execute_new_keygen()
		  }
	}
}