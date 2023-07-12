use dkg_primitives::types::DKGError;
use itertools::Itertools;
use parking_lot::Mutex;
use std::sync::Arc;
use wsts::common::PolyCommitment;

pub async fn dkg_async(
	signers: Arc<Mutex<Vec<wsts::v2::Party>>>,
) -> Result<Vec<PolyCommitment>, DKGError> {
	tokio::task::spawn_blocking(move || {
		let rng = &mut rand::thread_rng();
		wsts::v2::test_helpers::dkg(&mut signers.lock(), rng)
	})
	.await
	.map_err(|err| DKGError::CriticalError { reason: err.to_string() })?
	.map_err(|err| DKGError::GenericError {
		reason: err.into_values().map(|r| r.to_string()).join(", "),
	})
}

/// Returns a Vec of indices that denotes which indexes within the public key vector
/// are owned by which party.
///
/// E.g., if n=4 and k=10,
///
/// let party_key_ids: Vec<Vec<u32>> = [
///     [0, 1, 2].to_vec(),
///     [3, 4].to_vec(),
///     [5, 6, 7].to_vec(),
///     [8, 9].to_vec(),
/// ]
///
/// In the above case, we go up from 0..=9 possible key ids since k=10, and
/// we have 4 grouping since n=4. We need to generalize this below
#[allow(dead_code)]
fn generate_party_key_ids(n: u32, k: u32) -> Vec<Vec<u32>> {
	let mut result = Vec::with_capacity(n as usize);
	let ids_per_party = k / n;
	let mut start = 0;

	for _ in 0..n {
		let end = start + ids_per_party;
		let ids = (start..end).collect();
		result.push(ids);
		start = end;
	}

	result
}

#[cfg(test)]
mod tests {
	use crate::wsts_proto::generate_party_key_ids;
	use parking_lot::Mutex;
	use std::sync::Arc;

	#[tokio::test]
	async fn test_n3t2k10_raw() {
		test_inner::<10, 9, 10>().await;
	}

	async fn test_inner<const N: u32, const T: u32, const K: u32>() {
		dkg_logging::setup_log();
		assert_eq!(K % N, 0); // Enforce that each party owns the same number of keys
		assert_ne!(K, 0); // Enforce that K is not zero
		assert!(N > T);

		// Each node creates their own party
		let mut parties = Vec::new();
		let rng = &mut rand::thread_rng();
		let party_key_ids: Vec<Vec<u32>> = generate_party_key_ids(N, K);
		dkg_logging::info!("party_key_ids: {:?}", party_key_ids);
		// In reality, the idx below would be our index in the best authorities, starting from zero
		for idx in 0..N {
			// See https://github.com/Trust-Machines/wsts/blob/037e2eb4105cf9f9b1c034ee5c1540a40123b530/src/v2.rs#L515
			// for generating the party key IDS
			let key_indexes_owned_by_this_party = party_key_ids[idx as usize].clone();
			parties.push(wsts::v2::Party::new(idx, &key_indexes_owned_by_this_party, N, K, T, rng));

			// In the real implementation, each party would gossip their party.save() value to each
			// other which implements Serialize+Deserialize
		}

		// Assume each node has reconstructed the "parties" vector from the gossip round prior
		// Now, run the dkg compute to get the public key
		let parties = Arc::new(Mutex::new(parties));
		// Test the DKG
		let public_key = crate::wsts_proto::dkg_async(parties.clone()).await.unwrap();
		assert_eq!(public_key.len(), N as usize);

		// Test the signing over an arbitrary message
		let msg = b"Hello, world!";

		// Start by choosing signers. Since our indexes, in reality, will be based on the set of
		// best authorities, we will choose the best of the best of authorities, so from 0..T
		let mut signers = parties.lock().clone().into_iter().take(T as usize).collect::<Vec<_>>();

		// Setup the signature aggregator
		let mut sig_agg = wsts::v2::SignatureAggregator::new(K, T, public_key.clone()).unwrap();

		// Each node will sign the message then broadcast their signature share to the other nodes
		let (nonces, sig_shares, key_ids) =
			wsts::v2::test_helpers::sign(msg.as_slice(), &mut signers, rng);

		// Finally, each node will conclude
		match sig_agg.sign(msg.as_slice(), &nonces, &sig_shares, &key_ids) {
			Ok(sig) => {
				dkg_logging::info!(target: "dkg", "Successfully signed! Signature: R={:?}, z={:?}", sig.R, sig.z);
			},
			Err(err) => dkg_logging::error!(target: "dkg", "Error signing: {err:?}"),
		}
	}
}
