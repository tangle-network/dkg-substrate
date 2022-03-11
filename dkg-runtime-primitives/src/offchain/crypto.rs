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
use sp_core::sr25519::Signature as Sr25519Signature;
use sp_runtime::{
	app_crypto::{app_crypto, sr25519},
	key_types::ACCOUNT,
	traits::Verify,
	MultiSignature, MultiSigner,
};
app_crypto!(sr25519, ACCOUNT);

pub struct OffchainAuthId;

impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for OffchainAuthId {
	type RuntimeAppPublic = Public;
	type GenericSignature = sp_core::sr25519::Signature;
	type GenericPublic = sp_core::sr25519::Public;
}

impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature>
	for OffchainAuthId
{
	type RuntimeAppPublic = Public;
	type GenericSignature = sp_core::sr25519::Signature;
	type GenericPublic = sp_core::sr25519::Public;
}
