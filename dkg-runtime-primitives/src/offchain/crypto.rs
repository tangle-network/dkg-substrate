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