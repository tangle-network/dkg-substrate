use dkg_mock_blockchain::{
	transport::ProtocolPacket, FinalityNotification, ImportNotification, MockBlockChainEvent,
	TestBlock,
};
use dkg_runtime_primitives::crypto::AuthorityId;
use futures::{SinkExt, StreamExt};
use hash_db::HashDB;
use parking_lot::RwLock;
use sc_client_api::{AuxStore, BlockchainEvents, HeaderBackend};
use sc_network::PeerId;
use sc_utils::mpsc::*;
use sp_api::{
	offchain::storage::InMemOffchainStorage, ApiExt, AsTrieBackend, BlockId, BlockT,
	ProvideRuntimeApi, StateBackend,
};
use sp_runtime::traits::BlakeTwo256;
use sp_state_machine::{backend::Consolidate, *};
use sp_trie::HashDBT;
use std::{collections::HashMap, sync::Arc};
use tokio::net::ToSocketAddrs;

/// When peers use a Client, the streams they receive are suppose
/// to come from the BlockChain. However, for testing purposes, we will mock
/// the blockchain and thus send pseudo notifications to subscribing clients.
/// To allow flexibility in the design, each [`MockClient`] will need to connect
/// to a MockBlockchain service that periodically sends notifications to all connected
/// clients.
pub struct MockClient {}

#[derive(Clone)]
pub struct TestBackend {
	inner: TestBackendState,
}

#[derive(Clone)]
pub struct TestBackendState {
	finality_stream: Arc<MultiSubscribableStream<FinalityNotification<TestBlock>>>,
	import_stream: Arc<MultiSubscribableStream<ImportNotification<TestBlock>>>,
	api: DummyApi,
	offchain_storage: InMemOffchainStorage,
}

impl TestBackend {
	pub async fn connect<T: ToSocketAddrs>(
		mock_bc_addr: T,
		peer_id: PeerId,
		api: DummyApi,
	) -> std::io::Result<Self> {
		dkg_logging::info!(target: "dkg", "0. Setting up orchestrator<=>DKG communications for peer {peer_id:?}");
		let socket = tokio::net::TcpStream::connect(mock_bc_addr).await?;

		let this = TestBackend {
			inner: TestBackendState {
				finality_stream: Arc::new(MultiSubscribableStream::new()),
				import_stream: Arc::new(MultiSubscribableStream::new()),
				api,
				offchain_storage: Default::default(),
			},
		};

		let this_for_orchestrator_rx = this.clone();
		let task = async move {
			dkg_logging::info!(target: "dkg", "Complete: orchestrator<=>DKG communications for peer {peer_id:?}");
			let (mut tx, mut rx) =
				dkg_mock_blockchain::transport::bind_transport::<TestBlock>(socket);
			while let Some(packet) = rx.next().await {
				match packet {
					ProtocolPacket::InitialHandshake => {
						// pong back the handshake response
						tx.send(ProtocolPacket::InitialHandshakeResponse {
							peer_id: peer_id.clone(),
						})
						.await
						.unwrap();
					},
					ProtocolPacket::BlockChainToClient { event } => match event {
						MockBlockChainEvent::FinalityNotification { notification } => {
							this_for_orchestrator_rx.inner.finality_stream.send(notification);
						},
						MockBlockChainEvent::ImportNotification { notification } => {
							this_for_orchestrator_rx.inner.import_stream.send(notification);
						},
						MockBlockChainEvent::TestCase { trace_id, test } => {
							dkg_logging::warn!(target: "dkg", "The client is doing nothing right now for testcases")
						},
					},
					ProtocolPacket::Halt => {
						dkg_logging::info!(target: "dkg", "Received HALT command from the orchestrator");
						return
					},

					packet => {
						panic!("Received unexpected packet: {packet:?}")
					},
				}
			}

			panic!("The connection to the MockBlockchain died")
		};

		tokio::task::spawn(task);

		Ok(this)
	}
}

// each stream may be called multiple times. As such, we need to keep track of each subscriber
// and broadcast as necessary
struct MultiSubscribableStream<T> {
	inner: parking_lot::RwLock<Vec<TracingUnboundedSender<T>>>,
}

impl<T: Clone> MultiSubscribableStream<T> {
	pub fn new() -> Self {
		Self { inner: parking_lot::RwLock::new(vec![]) }
	}

	pub fn subscribe(&self) -> TracingUnboundedReceiver<T> {
		let (tx, rx) = tracing_unbounded("__inner_label");
		let mut lock = self.inner.write();
		lock.push(tx);
		rx
	}

	pub fn send(&self, t: T) {
		let lock = self.inner.read();
		assert!(!lock.is_empty());
		for tx in lock.iter() {
			if let Err(err) = tx.unbounded_send(t.clone()) {
				dkg_logging::error!(target: "dkg", "Error while sending through MultiSubscribableStream: {err:?}")
			}
		}
	}
}

impl BlockchainEvents<TestBlock> for TestBackend {
	fn finality_notification_stream(&self) -> sc_client_api::FinalityNotifications<TestBlock> {
		self.inner.finality_stream.subscribe()
	}

	fn import_notification_stream(&self) -> sc_client_api::ImportNotifications<TestBlock> {
		self.inner.import_stream.subscribe()
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
		Some(self.inner.offchain_storage.clone())
	}

	fn state_at(&self, hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<Self::State> {
		todo!()
	}

	fn revert(
		&self,
		n: sp_api::NumberFor<TestBlock>,
		revert_finalized: bool,
	) -> sp_blockchain::Result<(
		sp_api::NumberFor<TestBlock>,
		std::collections::HashSet<<TestBlock as BlockT>::Hash>,
	)> {
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
	fn header(
		&self,
		id: sp_api::BlockId<TestBlock>,
	) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Header>> {
		todo!()
	}

	fn info(&self) -> sp_blockchain::Info<TestBlock> {
		todo!()
	}

	fn status(
		&self,
		id: sp_api::BlockId<TestBlock>,
	) -> sp_blockchain::Result<sp_blockchain::BlockStatus> {
		todo!()
	}

	fn number(
		&self,
		hash: <TestBlock as BlockT>::Hash,
	) -> sp_blockchain::Result<Option<<<TestBlock as BlockT>::Header as sp_api::HeaderT>::Number>> {
		todo!()
	}

	fn hash(
		&self,
		number: sp_api::NumberFor<TestBlock>,
	) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Hash>> {
		todo!()
	}
}

impl ProvideRuntimeApi<TestBlock> for TestBackend {
	type Api = DummyApi;
	fn runtime_api(&self) -> sp_api::ApiRef<Self::Api> {
		sp_api::ApiRef::from(self.inner.api.clone())
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

#[derive(Clone)]
pub struct DummyApi {
	inner: Arc<RwLock<DummyApiInner>>,
}

pub struct DummyApiInner {
	keygen_t: u16,
	keygen_n: u16,
	signing_t: u16,
	signing_n: u16,
	// maps: block number => list of authorities for that block
	authority_sets: HashMap<u64, Vec<AuthorityId>>,
	dkg_keys: HashMap<dkg_runtime_primitives::AuthoritySetId, Vec<u8>>,
}

impl DummyApi {
	pub fn new(keygen_t: u16, keygen_n: u16, signing_t: u16, signing_n: u16) -> Self {
		Self {
			inner: Arc::new(RwLock::new(DummyApiInner {
				keygen_t,
				keygen_n,
				signing_t,
				signing_n,
				authority_sets: HashMap::new(),
				dkg_keys: HashMap::new(),
			})),
		}
	}

	fn block_id_to_u64(&self, input: &BlockId<TestBlock>) -> u64 {
		match input {
			BlockId::Number(number) => *number,
			// TODO: Make sure this produces the correct value!
			BlockId::Hash(hash) => hash.to_low_u64_be(),
		}
	}
}

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

	fn storage_hash(
		&self,
		key: &[u8],
	) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
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
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord,
	{
		todo!()
	}

	fn child_storage_root<'a>(
		&self,
		child_info: &sc_client_api::ChildInfo,
		delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, bool, Self::Transaction)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord,
	{
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

	fn execute_in_transaction<F: FnOnce(&Self) -> sp_api::TransactionOutcome<R>, R>(
		&self,
		call: F,
	) -> R
	where
		Self: Sized,
	{
		todo!()
	}

	fn has_api<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized,
	{
		todo!()
	}

	fn has_api_with<A: sp_api::RuntimeApiInfo + ?Sized, P: Fn(u32) -> bool>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
		pred: P,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized,
	{
		todo!()
	}

	fn api_version<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		at: &sp_api::BlockId<TestBlock>,
	) -> Result<Option<u32>, sp_api::ApiError>
	where
		Self: Sized,
	{
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
		Self: Sized,
	{
		todo!()
	}
}

impl AsTrieBackend<BlakeTwo256, Vec<u8>> for DummyStateBackend {
	type TrieBackendStorage = Self;

	fn as_trie_backend(
		&self,
	) -> &sp_api::TrieBackend<Self::TrieBackendStorage, BlakeTwo256, Vec<u8>> {
		todo!()
	}
}

#[derive(Default)]
pub struct DummyOverlay;
impl HashDBT<BlakeTwo256, Vec<u8>> for DummyOverlay {
	fn get(
		&self,
		_: &<BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
	) -> std::option::Option<Vec<u8>> {
		todo!()
	}
	fn contains(
		&self,
		_: &<BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
	) -> bool {
		todo!()
	}
	fn insert(
		&mut self,
		_: (&[u8], std::option::Option<u8>),
		_: &[u8],
	) -> <BlakeTwo256 as sp_core::Hasher>::Out {
		todo!()
	}
	fn emplace(
		&mut self,
		_: <BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
		_: Vec<u8>,
	) {
		todo!()
	}
	fn remove(
		&mut self,
		_: &<BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
	) {
		todo!()
	}
}

impl hash_db::AsHashDB<BlakeTwo256, Vec<u8>> for DummyOverlay {
	fn as_hash_db(&self) -> &dyn HashDB<BlakeTwo256, Vec<u8>> {
		todo!()
	}
	fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn HashDB<BlakeTwo256, Vec<u8>> + 'a) {
		todo!()
	}
}

impl Consolidate for DummyOverlay {
	fn consolidate(&mut self, _: Self) {
		todo!()
	}
}

impl sp_state_machine::TrieBackendStorage<BlakeTwo256> for DummyStateBackend {
	type Overlay = DummyOverlay;
	fn get(
		&self,
		_: &<BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
	) -> Result<std::option::Option<Vec<u8>>, std::string::String> {
		todo!()
	}
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

	fn update_cache(
		&mut self,
		cache: std::collections::HashMap<sp_blockchain::well_known_cache_keys::Id, Vec<u8>>,
	) {
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
		I: IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>,
	{
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

	fn update_transaction_index(
		&mut self,
		index: Vec<IndexOperation>,
	) -> sp_blockchain::Result<()> {
		todo!()
	}
}

impl AsTrieBackend<BlakeTwo256> for DummyStateBackend {
	type TrieBackendStorage = Self;

	fn as_trie_backend(
		&self,
	) -> &sp_api::TrieBackend<
		Self::TrieBackendStorage,
		BlakeTwo256,
		sp_trie::cache::LocalTrieCache<BlakeTwo256>,
	> {
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
		fn __runtime_api_internal_call_api_at(
			&self,
			_: &BlockId<TestBlock>,
			_: ExecutionContext,
			_: Vec<u8>,
			_: &dyn Fn(RuntimeVersion) -> &'static str,
		) -> ApiResult<Vec<u8>> {
			Ok(vec![])
		}
	}

	impl dkg_primitives::DKGApi<TestBlock, AuthorityId, sp_api::NumberFor<TestBlock>> for DummyApi {
		fn __runtime_api_internal_call_api_at(
			&self,
			_: &BlockId<TestBlock>,
			_: ExecutionContext,
			_: Vec<u8>,
			_: &dyn Fn(RuntimeVersion) -> &'static str,
		) -> ApiResult<Vec<u8>> {
			// This function for this dummy implementation does nothing
			Ok(vec![])
		}

		fn authority_set(
			&self,
			block: &BlockId<TestBlock>,
		) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId>> {
			let number = self.block_id_to_u64(block);
			dkg_logging::info!(target: "dkg", "Getting authority set for block {number}");
			let authorities = self.inner.read().authority_sets.get(&number).unwrap().clone();
			let authority_set_id = number;

			Ok(dkg_runtime_primitives::AuthoritySet { authorities, id: authority_set_id })
		}

		fn queued_authority_set(
			&self,
			id: &BlockId<TestBlock>,
		) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId>> {
			let next_id = BlockId::Number(self.block_id_to_u64(id) + 1);
			self.authority_set(&next_id)
		}

		fn signature_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			Ok(self.inner.read().signing_t)
		}

		fn keygen_threshold(&self, _: &BlockId<TestBlock>) -> ApiResult<u16> {
			Ok(self.inner.read().keygen_t)
		}

		fn next_signature_threshold(&self, block: &BlockId<TestBlock>) -> ApiResult<u16> {
			// TODO: proper implementation
			self.signature_threshold(block)
		}

		fn next_keygen_threshold(&self, block: &BlockId<TestBlock>) -> ApiResult<u16> {
			// TODO: proper implementation
			self.next_keygen_threshold(block)
		}

		fn should_refresh(
			&self,
			_: &BlockId<TestBlock>,
			block_number: BlockNumber,
		) -> ApiResult<bool> {
			todo!()
		}

		fn next_dkg_pub_key(
			&self,
			_: &BlockId<TestBlock>,
		) -> ApiResult<Option<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)>> {
			todo!()
		}

		fn next_pub_key_sig(&self, _: &BlockId<TestBlock>) -> ApiResult<Option<Vec<u8>>> {
			todo!()
		}

		fn dkg_pub_key(
			&self,
			block: &BlockId<TestBlock>,
		) -> ApiResult<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)> {
			let number = self.block_id_to_u64(block);
			if number == 0 {
				return Ok((number, vec![]))
			}
			dkg_logging::info!(target: "dkg", "Getting authority set for block {number}");
			let pub_key = self.inner.read().dkg_keys.get(&number).unwrap().clone();
			let authority_set_id = number;
			Ok((authority_set_id, pub_key))
		}

		fn get_best_authorities(
			&self,
			id: &BlockId<TestBlock>,
		) -> ApiResult<Vec<(u16, AuthorityId)>> {
			let read = self.inner.read();
			let id = self.block_id_to_u64(id);
			Ok(read
				.authority_sets
				.get(&id)
				.unwrap()
				.iter()
				.enumerate()
				.map(|(idx, auth)| (idx as u16 + 1, auth.clone()))
				.collect())
		}

		fn get_next_best_authorities(
			&self,
			id: &BlockId<TestBlock>,
		) -> ApiResult<Vec<(u16, AuthorityId)>> {
			let next_id = BlockId::Number(self.block_id_to_u64(id) + 1);
			self.get_best_authorities(&next_id)
		}

		fn get_current_session_progress(
			&self,
			_: &BlockId<TestBlock>,
			_block_number: BlockNumber,
		) -> ApiResult<Option<Permill>> {
			Ok(None)
		}

		fn get_unsigned_proposals(
			&self,
			_: &BlockId<TestBlock>,
		) -> ApiResult<Vec<UnsignedProposal>> {
			todo!()
			//DKGProposalHandler::get_unsigned_proposals()
		}

		fn get_max_extrinsic_delay(
			&self,
			_: &BlockId<TestBlock>,
			block_number: BlockNumber,
		) -> ApiResult<BlockNumber> {
			todo!()
		}

		fn get_authority_accounts(
			&self,
			_: &BlockId<TestBlock>,
		) -> ApiResult<(Vec<AccountId>, Vec<AccountId>)> {
			todo!()
			//Ok((DKG::current_authorities_accounts(), DKG::next_authorities_accounts()))
		}

		fn get_reputations(
			&self,
			_: &BlockId<TestBlock>,
			authorities: Vec<AuthorityId>,
		) -> ApiResult<Vec<(AuthorityId, Reputation)>> {
			todo!()
			//Ok(authorities.iter().map(|a| (a.clone(), DKG::authority_reputations(a))).collect())
		}

		fn get_keygen_jailed(
			&self,
			_: &BlockId<TestBlock>,
			set: Vec<AuthorityId>,
		) -> ApiResult<Vec<AuthorityId>> {
			todo!()
			//Ok(set.iter().filter(|a|
			// pallet_dkg_metadata::JailedKeygenAuthorities::<Runtime>::contains_key(a)).cloned().
			// collect())
		}

		fn get_signing_jailed(
			&self,
			_: &BlockId<TestBlock>,
			set: Vec<AuthorityId>,
		) -> ApiResult<Vec<AuthorityId>> {
			todo!()
			//Ok(set.iter().filter(|a|
			// pallet_dkg_metadata::JailedSigningAuthorities::<Runtime>::contains_key(a)).cloned().
			// collect())
		}

		fn refresh_nonce(&self, _: &BlockId<TestBlock>) -> ApiResult<u32> {
			todo!()
		}

		fn should_execute_new_keygen(&self, _: &BlockId<TestBlock>) -> ApiResult<bool> {
			todo!()
		}
	}
}

pub use mock_gossip::InMemoryGossipEngine;

pub mod mock_gossip {
	use crate::gossip_engine::GossipEngineIface;
	use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
	use std::collections::HashMap;
	pub type PeerId = sc_network::PeerId;
	use dkg_primitives::types::{DKGError, SignedDKGMessage};
	use dkg_runtime_primitives::crypto::AuthorityId;
	use futures::Stream;
	use parking_lot::Mutex;
	use std::{collections::VecDeque, pin::Pin, sync::Arc};

	use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};

	use super::MultiSubscribableStream;
	use dkg_runtime_primitives::{crypto, KEY_TYPE};

	#[derive(Clone)]
	pub struct InMemoryGossipEngine {
		clients: Arc<Mutex<HashMap<PeerId, VecDeque<SignedDKGMessage<AuthorityId>>>>>,
		notifier: Arc<Mutex<HashMap<PeerId, MultiSubscribableStream<()>>>>,
		this_peer: Option<PeerId>,
		this_peer_public_key: Option<AuthorityId>,
		// Maps Peer IDs to public keys
		mapping: Arc<Mutex<HashMap<PeerId, AuthorityId>>>,
	}

	impl InMemoryGossipEngine {
		pub fn new() -> Self {
			Self {
				clients: Arc::new(Mutex::new(Default::default())),
				notifier: Arc::new(Mutex::new(Default::default())),
				this_peer: None,
				this_peer_public_key: None,
				mapping: Arc::new(Mutex::new(Default::default())),
			}
		}

		// generates a new PeerId internally and adds to the hashmap
		pub fn clone_for_new_peer(
			&self,
			dummy_api: &super::DummyApi,
			n_blocks: u64,
			keyring: crate::keyring::Keyring,
			key_store: &dyn SyncCryptoStore,
		) -> Self {
			let public_key: crypto::Public = SyncCryptoStore::ecdsa_generate_new(
				&*key_store,
				KEY_TYPE,
				Some(&keyring.to_seed()),
			)
			.ok()
			.unwrap()
			.into();

			let this_peer = PeerId::random();
			let stream = MultiSubscribableStream::new();
			self.mapping.lock().insert(this_peer.clone(), public_key.clone());
			self.clients.lock().insert(this_peer.clone(), Default::default());
			self.notifier.lock().insert(this_peer.clone(), stream);

			// by default, add this peer to the best authorities
			// TODO: make the configurable
			let mut lock = dummy_api.inner.write();
			// add +1 to allow calls for queued_authorities at block=n_blocks to not fail
			for x in 0..n_blocks + 1 {
				lock.authority_sets.entry(x).or_default().push(public_key.clone());
			}

			Self {
				clients: self.clients.clone(),
				notifier: self.notifier.clone(),
				this_peer: Some(this_peer),
				this_peer_public_key: Some(public_key),
				mapping: self.mapping.clone(),
			}
		}

		pub fn peer_id(&self) -> (&PeerId, &AuthorityId) {
			(self.this_peer.as_ref().unwrap(), self.this_peer_public_key.as_ref().unwrap())
		}
	}

	impl GossipEngineIface for InMemoryGossipEngine {
		type Clock = u128;
		/// Send a DKG message to a specific peer.
		fn send(
			&self,
			recipient: PeerId,
			message: SignedDKGMessage<AuthorityId>,
		) -> Result<(), DKGError> {
			let mut clients = self.clients.lock();
			let tx = clients
				.get_mut(&recipient)
				.ok_or_else(|| error(format!("Peer {recipient:?} does not exist")))?;
			tx.push_back(message);

			// notify the receiver
			self.notifier.lock().get(&recipient).unwrap().send(());
			Ok(())
		}

		/// Send a DKG message to all peers.
		fn gossip(&self, message: SignedDKGMessage<AuthorityId>) -> Result<(), DKGError> {
			let mut clients = self.clients.lock();
			let notifiers = self.notifier.lock();
			let (this_peer, _) = self.peer_id();

			for (peer_id, tx) in clients.iter_mut() {
				if peer_id != this_peer {
					tx.push_back(message.clone());
				}
			}

			for (peer_id, notifier) in notifiers.iter() {
				if peer_id != this_peer {
					notifier.send(());
				}
			}

			Ok(())
		}
		/// A stream that sends messages when they are ready to be polled from the message queue.
		fn message_available_notification(&self) -> Pin<Box<dyn Stream<Item = ()> + Send>> {
			let (this_peer, _) = self.peer_id();
			let rx = self.notifier.lock().get(this_peer).unwrap().subscribe();
			Box::pin(rx) as _
		}
		/// Peek the front of the message queue.
		///
		/// Note that this will not remove the message from the queue, it will only return it. For
		/// removing the message from the queue, use `acknowledge_last_message`.
		///
		/// Returns `None` if there are no messages in the queue.
		fn peek_last_message(&self) -> Option<SignedDKGMessage<AuthorityId>> {
			let (this_peer, _) = self.peer_id();
			let clients = self.clients.lock();
			clients.get(this_peer).unwrap().front().cloned()
		}
		/// Acknowledge the last message (the front of the queue) and mark it as processed, then
		/// removes it from the queue.
		fn acknowledge_last_message(&self) {
			let (this_peer, _) = self.peer_id();
			let mut clients = self.clients.lock();
			clients.get_mut(this_peer).unwrap().pop_front();
		}

		/// Clears the Message Queue.
		fn clear_queue(&self) {
			let (this_peer, _) = self.peer_id();
			let mut clients = self.clients.lock();
			clients.get_mut(this_peer).unwrap().clear();
		}
	}

	fn error<T: std::fmt::Debug>(err: T) -> DKGError {
		DKGError::GenericError { reason: format!("{err:?}") }
	}
}
