use crate::{
	async_protocols::{remote::AsyncProtocolRemote, KeygenPartyId},
	gossip_engine::GossipEngineIface,
	utils::SendFuture,
	worker::{DKGWorker, ProtoStageType},
	Client,
};
use async_trait::async_trait;
use dkg_primitives::types::{DKGError, SSID};
use dkg_runtime_primitives::{
	crypto::{AuthorityId, Public},
	BatchId, DKGApi, MaxAuthorities, MaxProposalLength, MaxProposalsInBatch, SessionId,
	StoredUnsignedProposalBatch,
};
use mp_ecdsa::MpEcdsaDKG;
use parking_lot::RwLock;
use sc_client_api::Backend;
use sp_runtime::traits::{Block, NumberFor};
use std::{marker::PhantomData, pin::Pin, sync::Arc};
use wt_frost::WTFrostDKG;

pub mod mp_ecdsa;
pub mod wt_frost;

/// Setup parameters for the Keygen protocol
pub enum KeygenProtocolSetupParameters<B: Block> {
	MpEcdsa {
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		associated_block: NumberFor<B>,
		threshold: u16,
		stage: ProtoStageType,
		keygen_protocol_hash: [u8; 32],
	},
	WTFrost {},
}

/// Setup parameters for the Signing protocol
pub enum SigningProtocolSetupParameters<B: Block> {
	MpEcdsa {
		best_authorities: Vec<(KeygenPartyId, Public)>,
		authority_public_key: Public,
		party_i: KeygenPartyId,
		session_id: SessionId,
		threshold: u16,
		stage: ProtoStageType,
		unsigned_proposal_batch: StoredUnsignedProposalBatch<
			BatchId,
			MaxProposalLength,
			MaxProposalsInBatch,
			NumberFor<B>,
		>,
		signing_set: Vec<KeygenPartyId>,
		associated_block_id: NumberFor<B>,
		ssid: SSID,
		unsigned_proposal_hash: [u8; 32],
	},
	WTFrost {},
}

/// A type which is used directly by the Job Manager to initialize and manage the DKG protocol
pub type ProtocolInitReturn<B> =
	(AsyncProtocolRemote<NumberFor<B>>, Pin<Box<dyn SendFuture<'static, ()>>>);

#[async_trait]
/// Generalizes the DKGWorker::initialize_keygen_protocol and DKGWorker::initialize_signing_protocol
/// Also includes two functions which are used for determining whether a DKG can handle the request
/// parameters, which is used in the [`DKGModules`] implementation
pub trait DKG<B: Block>: Send + Sync {
	async fn initialize_keygen_protocol(
		&self,
		params: KeygenProtocolSetupParameters<B>,
	) -> Option<ProtocolInitReturn<B>>;
	async fn initialize_signing_protocol(
		&self,
		params: SigningProtocolSetupParameters<B>,
	) -> Result<ProtocolInitReturn<B>, DKGError>;
	fn can_handle_keygen_request(&self, params: &KeygenProtocolSetupParameters<B>) -> bool;
	fn can_handle_signing_request(&self, params: &SigningProtocolSetupParameters<B>) -> bool;
}

/// Holds a list of DKGs that can be used at runtime
pub struct DKGModules<B: Block, BE, C, GE> {
	dkgs: Arc<RwLock<Vec<Arc<dyn DKG<B>>>>>,
	_pd: PhantomData<(BE, C, GE)>,
}

impl<B, BE, C, GE> DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + Unpin + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
	C::Api: DKGApi<B, AuthorityId, NumberFor<B>, MaxProposalLength, MaxAuthorities>,
{
	/// Loads the default DKG modules internally to be available at runtime
	pub fn initialize(&self, dkg_worker: DKGWorker<B, BE, C, GE>) {
		*self.dkgs.write() = vec![
			Arc::new(MpEcdsaDKG { dkg_worker: dkg_worker.clone() }),
			Arc::new(WTFrostDKG { dkg_worker }),
		]
	}

	/// Given a set of parameters, returns the keygen protocol initializer which can handle the
	/// request
	pub fn get_keygen_protocol(
		&self,
		params: &KeygenProtocolSetupParameters<B>,
	) -> Option<Arc<dyn DKG<B>>> {
		self.dkgs
			.read()
			.iter()
			.find(|dkg| dkg.can_handle_keygen_request(params))
			.cloned()
	}

	/// Given a set of parameters, returns the signing protocol initializer which can handle the
	/// request
	pub fn get_signing_protocol(
		&self,
		params: &SigningProtocolSetupParameters<B>,
	) -> Option<Arc<dyn DKG<B>>> {
		self.dkgs
			.read()
			.iter()
			.find(|dkg| dkg.can_handle_signing_request(params))
			.cloned()
	}
}

impl<B, BE, C, GE> Default for DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
{
	fn default() -> Self {
		Self { dkgs: Arc::new(RwLock::new(vec![])), _pd: PhantomData }
	}
}

impl<B, BE, C, GE> Clone for DKGModules<B, BE, C, GE>
where
	B: Block,
	BE: Backend<B> + 'static,
	C: Client<B, BE> + 'static,
	GE: GossipEngineIface,
{
	fn clone(&self) -> Self {
		Self { dkgs: self.dkgs.clone(), _pd: PhantomData }
	}
}
