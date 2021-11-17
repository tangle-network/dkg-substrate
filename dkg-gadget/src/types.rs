use sp_runtime::traits::{Block, Hash, Header};

/// Gossip engine dkg messages topic
pub(crate) fn dkg_topic<B: Block>() -> B::Hash
where
	B: Block,
{
	<<B::Header as Header>::Hashing as Hash>::hash(b"dkg")
}
