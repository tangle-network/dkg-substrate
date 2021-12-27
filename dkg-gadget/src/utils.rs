use crate::worker::ENGINE_ID;
use codec::Codec;
use dkg_primitives::{
	crypto::AuthorityId, rounds::MultiPartyECDSARounds, AuthoritySet, ConsensusLog, MmrRootHash,
};
use dkg_runtime_primitives::DKGPayloadKey;
use sp_api::{BlockT as Block, HeaderT};
use sp_runtime::generic::OpaqueDigestItemId;

pub fn find_index<B: Eq>(queue: &Vec<B>, value: &B) -> Option<usize> {
	for (i, v) in queue.iter().enumerate() {
		if value == v {
			return Some(i)
		}
	}
	None
}

pub fn validate_threshold(n: u16, t: u16) -> u16 {
	let max_thresh = n - 1;
	if t >= 1 && t <= max_thresh {
		return t
	}

	return max_thresh
}

pub fn set_up_rounds(
	authority_set: &AuthoritySet<AuthorityId>,
	public: &AuthorityId,
	thresh: u16,
	local_key_path: Option<std::path::PathBuf>,
	offline_stage_path: Option<std::path::PathBuf>,
) -> MultiPartyECDSARounds<DKGPayloadKey> {
	let party_inx = find_index::<AuthorityId>(&authority_set.authorities, public).unwrap() + 1;

	let n = authority_set.authorities.len();

	let rounds = MultiPartyECDSARounds::new(
		u16::try_from(party_inx).unwrap(),
		thresh,
		u16::try_from(n).unwrap(),
		authority_set.id.clone(),
		local_key_path,
		offline_stage_path,
	);

	rounds
}

/// Extract the MMR root hash from a digest in the given header, if it exists.
pub fn find_mmr_root_digest<B, Id>(header: &B::Header) -> Option<MmrRootHash>
where
	B: Block,
	Id: Codec,
{
	header.digest().logs().iter().find_map(|log| {
		match log.try_to::<ConsensusLog<Id>>(OpaqueDigestItemId::Consensus(&ENGINE_ID)) {
			Some(ConsensusLog::MmrRoot(root)) => Some(root),
			_ => None,
		}
	})
}

/// Scan the `header` digest log for a DKG validator set change. Return either the new
/// validator set or `None` in case no validator set change has been signaled.
pub fn find_authorities_change<B>(
	header: &B::Header,
) -> Option<(AuthoritySet<AuthorityId>, AuthoritySet<AuthorityId>)>
where
	B: Block,
{
	let id = OpaqueDigestItemId::Consensus(&ENGINE_ID);

	let filter = |log: ConsensusLog<AuthorityId>| match log {
		ConsensusLog::AuthoritiesChange {
			next_authorities: validator_set,
			next_queued_authorities,
		} => Some((validator_set, next_queued_authorities)),
		_ => None,
	};

	header.digest().convert_first(|l| l.try_to(id).and_then(filter))
}
