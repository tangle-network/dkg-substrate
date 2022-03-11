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
use frame_support::dispatch::DispatchResultWithPostInfo;
use sp_std::vec::Vec;

pub trait OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> {
	fn on_authority_set_changed(
		authority_set_id: AuthoritySetId,
		authorities: Vec<AuthorityId>,
	) -> ();
}

impl<AuthoritySetId, AuthorityId> OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> for () {
	fn on_authority_set_changed(
		_authority_set_id: AuthoritySetId,
		_authorities: Vec<AuthorityId>,
	) -> () {
	}
}

pub trait GetDKGPublicKey {
	fn dkg_key() -> Vec<u8>;
	fn previous_dkg_key() -> Vec<u8>;
}

/// A trait for when the DKG Public Key get changed.
///
/// This is used to notify the runtime that the DKG signer has changed.
/// for example, this could be used to know that we should trigger a re-signing of any pending
/// signed proposals to take into account the new DKG signer.
pub trait OnDKGPublicKeyChangeHandler<AuthoritySetId: Copy> {
	fn on_dkg_public_key_changed(
		authority_id: AuthoritySetId,
		dkg_public_key: Vec<u8>,
	) -> DispatchResultWithPostInfo;
}

// A helper macro that would generate the implementation of the trait for tuples
// for example: we can use (A, B, C) as value implementation and it will call A, B, C in order.
#[impl_trait_for_tuples::impl_for_tuples(5)]
impl<AuthoritySetId: Copy> OnDKGPublicKeyChangeHandler<AuthoritySetId> for Tuple5 {
	#[allow(clippy::redundant_clone)]
	fn on_dkg_public_key_changed(
		authority_id: AuthoritySetId,
		dkg_public_key: Vec<u8>,
	) -> DispatchResultWithPostInfo {
		for_tuples!( #( Tuple5::on_dkg_public_key_changed(authority_id, dkg_public_key.clone())?; )* );
		Ok(().into())
	}
}
