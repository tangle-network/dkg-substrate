use dkg_gadget::debug_logger::DebugLogger;
use dkg_mock_blockchain::{MutableBlockchain, TestBlock};
use dkg_runtime_primitives::{
	crypto::AuthorityId, MaxAuthorities, MaxProposalLength, StoredUnsignedProposalBatch,
	UnsignedProposal,
};
use hash_db::HashDB;
use parking_lot::RwLock;
use sp_api::{ApiExt, AsTrieBackend, BlockT, StateBackend, *};
use sp_core::bounded_vec::BoundedVec;
use sp_runtime::{testing::H256, traits::BlakeTwo256, Permill};
use sp_state_machine::*;
use sp_trie::HashDBT;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct DummyApi {
	pub inner: Arc<RwLock<DummyApiInner>>,
	logger: DebugLogger,
}

pub struct DummyApiInner {
	pub keygen_t: u16,
	#[allow(dead_code)]
	pub keygen_n: u16,
	pub signing_t: u16,
	#[allow(dead_code)]
	pub signing_n: u16,
	// maps: block number => list of authorities for that block
	pub authority_sets: HashMap<u64, BoundedVec<AuthorityId, MaxAuthorities>>,
	pub dkg_keys: HashMap<dkg_runtime_primitives::AuthoritySetId, Vec<u8>>,
	pub unsigned_proposals: Vec<
		dkg_runtime_primitives::StoredUnsignedProposalBatch<
			dkg_runtime_primitives::BatchId,
			dkg_runtime_primitives::MaxProposalLength,
			dkg_runtime_primitives::MaxProposalsInBatch,
			BlockNumber,
		>,
	>,
	pub should_execute_keygen: bool,
	pub blocks_per_session: u64,
	pub incrementing_batch_id: u32,
}

impl MutableBlockchain for DummyApi {
	fn set_unsigned_proposals(
		&self,
		propos: Vec<(UnsignedProposal<dkg_runtime_primitives::CustomU32Getter<10000>>, u64)>,
	) {
		// Use an incremented batch ID to avoid collision
		let mut lock = self.inner.write();
		let batch_id = lock.incrementing_batch_id;
		lock.incrementing_batch_id += 1;

		let batches = propos
			.iter()
			.map(|prop| StoredUnsignedProposalBatch {
				proposals: vec![prop.clone().0].try_into().unwrap(),
				batch_id,
				timestamp: 0,
			})
			.collect::<Vec<_>>();

		lock.unsigned_proposals = batches;
	}

	fn set_pub_key(&self, block_id: u64, key: Vec<u8>) {
		let header = sp_runtime::generic::Header::<u64, _>::new_from_number(block_id);
		let hash = header.hash();
		let session_id = self.block_id_to_session_id(&hash);
		self.inner.write().dkg_keys.insert(session_id, key);
	}

	fn set_should_execute_keygen(&self, should_execute: bool) {
		self.inner.write().should_execute_keygen = should_execute;
	}
}

impl DummyApi {
	pub fn new(
		keygen_t: u16,
		keygen_n: u16,
		signing_t: u16,
		signing_n: u16,
		logger: DebugLogger,
		blocks_per_session: u64,
	) -> Self {
		let mut dkg_keys = HashMap::new();
		// add a empty-key for the genesis block to drive the DKG forward
		dkg_keys.insert(0 as _, vec![]);

		Self {
			inner: Arc::new(RwLock::new(DummyApiInner {
				keygen_t,
				keygen_n,
				signing_t,
				signing_n,
				authority_sets: HashMap::new(),
				dkg_keys,
				unsigned_proposals: vec![],
				should_execute_keygen: false,
				blocks_per_session,
				incrementing_batch_id: 0,
			})),
			logger,
		}
	}

	fn block_id_to_session_id(&self, input: &H256) -> u64 {
		// this is hacky, but, it should suffice for now
		for x in 0..=u64::MAX {
			let header = sp_runtime::generic::Header::<u64, _>::new_from_number(x);
			let hash = header.hash();
			if &hash == input {
				// take x and divide by blocks_per_session
				let blocks_per_session = self.inner.read().blocks_per_session as f64;
				// if bps = 2
				// x = 0 => 0
				// x = 1 => 0
				// x = 2 => 1
				// x = 3 => 1
				// thus: floor(x / bps)
				return (x as f64 / blocks_per_session).floor() as u64
			}
		}

		unreachable!("block_id_to_u64: could not find block number for hash {}", input);
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

pub struct DummyRawIterator;

impl sp_state_machine::StorageIterator<BlakeTwo256> for DummyRawIterator {
	type Backend = DummyStateBackend;
	type Error = DummyError;

	fn next_key(&mut self, _backend: &Self::Backend) -> Option<Result<StorageKey, Self::Error>> {
		todo!()
	}

	fn next_pair(
		&mut self,
		_backend: &Self::Backend,
	) -> Option<Result<(StorageKey, StorageValue), Self::Error>> {
		todo!()
	}

	fn was_complete(&self) -> bool {
		todo!()
	}
}

impl StateBackend<BlakeTwo256> for DummyStateBackend {
	type Error = DummyError;

	type TrieBackendStorage = DummyStateBackend;

	type RawIter = DummyRawIterator;

	fn raw_iter(&self, _args: IterArgs) -> Result<Self::RawIter, Self::Error> {
		todo!()
	}

	fn storage(&self, _key: &[u8]) -> Result<Option<StorageValue>, Self::Error> {
		todo!()
	}

	fn storage_hash(
		&self,
		_key: &[u8],
	) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
		todo!()
	}

	fn child_storage(
		&self,
		_child_info: &sc_client_api::ChildInfo,
		_key: &[u8],
	) -> Result<Option<StorageValue>, Self::Error> {
		todo!()
	}

	fn child_storage_hash(
		&self,
		_child_info: &sc_client_api::ChildInfo,
		_key: &[u8],
	) -> Result<Option<<BlakeTwo256 as sp_api::Hasher>::Out>, Self::Error> {
		todo!()
	}

	fn next_storage_key(&self, _key: &[u8]) -> Result<Option<StorageKey>, Self::Error> {
		todo!()
	}

	fn next_child_storage_key(
		&self,
		_child_info: &sc_client_api::ChildInfo,
		_key: &[u8],
	) -> Result<Option<StorageKey>, Self::Error> {
		todo!()
	}

	fn storage_root<'a>(
		&self,
		_delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		_state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, BackendTransaction<BlakeTwo256>)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord,
	{
		todo!()
	}

	fn child_storage_root<'a>(
		&self,
		_child_info: &sc_client_api::ChildInfo,
		_delta: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
		_state_version: sp_api::StateVersion,
	) -> (<BlakeTwo256 as sp_api::Hasher>::Out, bool, BackendTransaction<BlakeTwo256>)
	where
		<BlakeTwo256 as sp_api::Hasher>::Out: Ord,
	{
		todo!()
	}

	fn pairs<'a>(
		&'a self,
		_args: IterArgs,
	) -> Result<PairsIter<'a, BlakeTwo256, Self::RawIter>, Self::Error> {
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
	fn execute_in_transaction<F: FnOnce(&Self) -> sp_api::TransactionOutcome<R>, R>(
		&self,
		_call: F,
	) -> R
	where
		Self: Sized,
	{
		todo!()
	}

	fn has_api<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		_at: H256,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized,
	{
		todo!()
	}

	fn has_api_with<A: sp_api::RuntimeApiInfo + ?Sized, P: Fn(u32) -> bool>(
		&self,
		_at: H256,
		_pred: P,
	) -> Result<bool, sp_api::ApiError>
	where
		Self: Sized,
	{
		todo!()
	}

	fn api_version<A: sp_api::RuntimeApiInfo + ?Sized>(
		&self,
		_at: H256,
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

	fn into_storage_changes<B: StateBackend<HashingFor<TestBlock>>>(
		&self,
		_backend: &B,
		_parent_hash: <TestBlock as BlockT>::Hash,
	) -> Result<sp_api::StorageChanges<TestBlock>, String>
	where
		Self: Sized,
	{
		todo!()
	}

	fn set_call_context(&mut self, _call_context: CallContext) {
		todo!()
	}

	fn register_extension<E: Extension>(&mut self, _extension: E) {
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

impl sp_state_machine::TrieBackendStorage<BlakeTwo256> for DummyStateBackend {
	fn get(
		&self,
		_: &<BlakeTwo256 as sp_core::Hasher>::Out,
		_: (&[u8], std::option::Option<u8>),
	) -> Result<std::option::Option<Vec<u8>>, std::string::String> {
		todo!()
	}
}

impl sc_client_api::BlockImportOperation<TestBlock> for DummyStateBackend {
	type State = Self;

	fn state(&self) -> sp_blockchain::Result<Option<&Self::State>> {
		todo!()
	}

	fn set_block_data(
		&mut self,
		_header: <TestBlock as BlockT>::Header,
		_body: Option<Vec<<TestBlock as BlockT>::Extrinsic>>,
		_indexed_body: Option<Vec<Vec<u8>>>,
		_justifications: Option<sp_runtime::Justifications>,
		_state: sc_client_api::NewBlockState,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn update_db_storage(
		&mut self,
		_update: BackendTransaction<HashingFor<TestBlock>>,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn set_genesis_state(
		&mut self,
		_storage: sp_runtime::Storage,
		_commit: bool,
		_state_version: sp_api::StateVersion,
	) -> sp_blockchain::Result<<TestBlock as BlockT>::Hash> {
		todo!()
	}

	fn reset_storage(
		&mut self,
		_storage: sp_runtime::Storage,
		_state_version: sp_api::StateVersion,
	) -> sp_blockchain::Result<<TestBlock as BlockT>::Hash> {
		todo!()
	}

	fn update_storage(
		&mut self,
		_update: StorageCollection,
		_child_update: ChildStorageCollection,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn insert_aux<I>(&mut self, _ops: I) -> sp_blockchain::Result<()>
	where
		I: IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>,
	{
		todo!()
	}

	fn mark_finalized(
		&mut self,
		_hash: <TestBlock as BlockT>::Hash,
		_justification: Option<sp_runtime::Justification>,
	) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn mark_head(&mut self, _hash: <TestBlock as BlockT>::Hash) -> sp_blockchain::Result<()> {
		todo!()
	}

	fn update_transaction_index(
		&mut self,
		_index: Vec<IndexOperation>,
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
		_: H256,
		_: Vec<u8>,
		_: &dyn Fn(RuntimeVersion) -> &'static str,
	) -> ApiResult<Vec<u8>> {
		Ok(vec![])
	}
}

impl
	dkg_primitives::DKGApi<
		TestBlock,
		AuthorityId,
		sp_api::NumberFor<TestBlock>,
		MaxProposalLength,
		MaxAuthorities,
	> for DummyApi
{
	fn __runtime_api_internal_call_api_at(
		&self,
		_: H256,
		_: Vec<u8>,
		_: &dyn Fn(RuntimeVersion) -> &'static str,
	) -> ApiResult<Vec<u8>> {
		// This function for this dummy implementation does nothing
		Ok(vec![])
	}

	fn authority_set(
		&self,
		block: H256,
	) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId, MaxAuthorities>> {
		let number = self.block_id_to_session_id(&block);
		self.logger.info(format!("Getting authority set for session_id = {number}"));
		let authorities = self.inner.read().authority_sets.get(&number).unwrap().clone();
		let authority_set_id = number;

		Ok(dkg_runtime_primitives::AuthoritySet { authorities, id: authority_set_id })
	}

	fn queued_authority_set(
		&self,
		id: H256,
	) -> ApiResult<dkg_runtime_primitives::AuthoritySet<AuthorityId, MaxAuthorities>> {
		let next_session_id = self.block_id_to_session_id(&id) + 1;
		self.logger
			.info(format!("Getting queued authority set for session_id = {next_session_id}"));
		let authorities = self.inner.read().authority_sets.get(&next_session_id).unwrap().clone();
		let authority_set_id = next_session_id;

		Ok(dkg_runtime_primitives::AuthoritySet { authorities, id: authority_set_id })
	}

	fn signature_threshold(&self, _: H256) -> ApiResult<u16> {
		Ok(self.inner.read().signing_t)
	}

	fn keygen_threshold(&self, _: H256) -> ApiResult<u16> {
		Ok(self.inner.read().keygen_t)
	}

	fn next_signature_threshold(&self, _block: H256) -> ApiResult<u16> {
		Ok(self.inner.read().signing_t)
	}

	fn next_keygen_threshold(&self, _block: H256) -> ApiResult<u16> {
		Ok(self.inner.read().keygen_t)
	}

	fn should_refresh(&self, _: H256, _block_number: BlockNumber) -> ApiResult<bool> {
		Ok(true)
	}

	fn next_dkg_pub_key(
		&self,
		id: H256,
	) -> ApiResult<Option<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)>> {
		let number = self.block_id_to_session_id(&id) + 1;
		self.logger.info(format!("Getting next authority set for block {number}"));
		if let Some(pub_key) = self.inner.read().dkg_keys.get(&number).cloned() {
			let authority_set_id = number;
			Ok(Some((authority_set_id, pub_key)))
		} else {
			Ok(None)
		}
	}

	fn next_pub_key_sig(&self, _: H256) -> ApiResult<Option<Vec<u8>>> {
		self.logger.error("unimplemented get_next_pub_key_sig".to_string());
		todo!()
	}

	fn dkg_pub_key(
		&self,
		block: H256,
	) -> ApiResult<(dkg_runtime_primitives::AuthoritySetId, Vec<u8>)> {
		let number = self.block_id_to_session_id(&block);
		self.logger.info(format!("Getting pub key for session_id = {number}"));
		let pub_key = self.inner.read().dkg_keys.get(&number).unwrap().clone();
		let authority_set_id = number;
		Ok((authority_set_id, pub_key))
	}

	fn get_best_authorities(&self, id: H256) -> ApiResult<Vec<(u16, AuthorityId)>> {
		let id = self.block_id_to_session_id(&id);
		let read = self.inner.read();
		Ok(read
			.authority_sets
			.get(&id)
			.unwrap()
			.iter()
			.enumerate()
			.map(|(idx, auth)| (idx as u16 + 1, auth.clone()))
			.collect())
	}

	fn get_next_best_authorities(&self, id: H256) -> ApiResult<Vec<(u16, AuthorityId)>> {
		let next_session_id = self.block_id_to_session_id(&id) + 1;
		let read = self.inner.read();
		Ok(read
			.authority_sets
			.get(&next_session_id)
			.unwrap()
			.iter()
			.enumerate()
			.map(|(idx, auth)| (idx as u16 + 1, auth.clone()))
			.collect())
	}

	fn get_current_session_progress(
		&self,
		_: H256,
		_block_number: BlockNumber,
	) -> ApiResult<Option<Permill>> {
		Ok(None)
	}

	fn get_unsigned_proposal_batches(
		&self,
		_hash: H256,
	) -> ApiResult<
		Vec<
			dkg_runtime_primitives::StoredUnsignedProposalBatch<
				dkg_runtime_primitives::BatchId,
				dkg_runtime_primitives::MaxProposalLength,
				dkg_runtime_primitives::MaxProposalsInBatch,
				BlockNumber,
			>,
		>,
	> {
		Ok(self.inner.read().unsigned_proposals.clone())
	}

	fn get_authority_accounts(&self, _: H256) -> ApiResult<(Vec<AccountId>, Vec<AccountId>)> {
		self.logger.error("unimplemented get_authority_accounts".to_string());
		todo!()
		//Ok((DKG::current_authorities_accounts(), DKG::next_authorities_accounts()))
	}

	fn get_reputations(
		&self,
		_: H256,
		_authorities: Vec<AuthorityId>,
	) -> ApiResult<Vec<(AuthorityId, Reputation)>> {
		self.logger.error("unimplemented get_repuations".to_string());
		todo!()
		//Ok(authorities.iter().map(|a| (a.clone(), DKG::authority_reputations(a))).collect())
	}

	fn get_keygen_jailed(&self, _: H256, _set: Vec<AuthorityId>) -> ApiResult<Vec<AuthorityId>> {
		Ok(vec![])
	}

	fn get_signing_jailed(&self, _: H256, _set: Vec<AuthorityId>) -> ApiResult<Vec<AuthorityId>> {
		Ok(vec![])
	}

	fn refresh_nonce(&self, _: H256) -> ApiResult<u32> {
		Ok(0)
	}

	fn should_execute_new_keygen(&self, _: H256) -> ApiResult<(bool, bool)> {
		Ok((self.inner.read().should_execute_keygen, false))
	}

	fn should_submit_proposer_vote(&self, _: H256) -> ApiResult<bool> {
		Ok(true)
	}
}
