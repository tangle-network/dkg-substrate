use dkg_runtime_primitives::TypedChainId;
use frame_support::ensure;
use sp_runtime::DispatchError;

use crate::{proof::evm::EvmStorageVerifierTrait, types::ProofData, Config, Error};

pub mod evm;
use evm::EvmStorageVerifier;

pub trait StorageVerifier<T: Config> {
	fn verify_log_entry(
		typed_chain_id: TypedChainId,
		proof: ProofData,
	) -> Result<Vec<u8>, DispatchError> {
		match typed_chain_id {
			TypedChainId::Evm(_) => {
				ensure!(proof.to_evm_proof().is_some(), Error::<T>::InvalidProofData);
				let evm_proof_data = proof.to_evm_proof().unwrap();
				EvmStorageVerifier::<T>::try_verify_log_entry(evm_proof_data.clone())
			},
			_ => Err(Error::<T>::InvalidTypedChainId)?,
		}
	}

	fn verify_trie_proof(
		_typed_chain_id: TypedChainId,
		_proof: ProofData,
	) -> Result<Vec<u8>, DispatchError> {
		Err(Error::<T>::InvalidTypedChainId)?
	}
}
