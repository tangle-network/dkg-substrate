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

//! let (tx, rx) = futures::channel() // incoming
//! let (tx1, rx2) = futures::channel() // outgoing
//! 
//! // Interface: pub fn new(state: SM, incoming: I, outgoing: O) -> Self
//! AsyncProtocol::new(
//!     state_machine,
//!     IncomingAsyncProtocolWrapper { receiver: rx },
//!     OutgoingAsyncProtocolWrapper { sender: tx1 },
//! ).run().await


use std::{task::{Context, Poll}, pin::Pin};

use codec::Encode;
use dkg_primitives::{types::{DKGError, DKGMessage, SignedDKGMessage, DKGMsgPayload}, crypto::Public, AuthoritySet};
use dkg_runtime_primitives::utils::to_slice_32;
use futures::{stream::Stream, Sink};
use log::error;
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::{keygen::Keygen};
use round_based::{Msg, async_runtime::AsyncProtocol, StateMachine};
use sp_core::sr25519;
use sp_runtime::{traits::Block, AccountId32};
use sc_client_api::Backend;
use crate::Client;

use crate::worker::DKGWorker;

pub struct IncomingAsyncProtocolWrapper<T> {
    pub receiver: futures::channel::mpsc::UnboundedReceiver<T>,
}

pub struct OutgoingAsyncProtocolWrapper<T: TransformOutgoing> {
    pub sender: futures::channel::mpsc::UnboundedSender<T::Output>,
}

pub trait TransformIncoming {
    type IncomingMapped;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32]) -> Result<Msg<Self::IncomingMapped>, DKGError> where Self: Sized;
}

impl TransformIncoming for SignedDKGMessage<Public> {
    type IncomingMapped = DKGMessage<Public>;
    fn transform(self, party_index: u16, active: &[AccountId32], next: &[AccountId32]) -> Result<Msg<Self::IncomingMapped>, DKGError> where Self: Sized {
        verify_signature_against_authorities(self, active, next).map(|body| {
            // let payload: DKGMsgPayload = body.payload;
            // match payload {
            //     // Keygen
            //     DKGMsgPayload::Keygen(msg) => {
            //         let keygen_msg = msg.keygen_msg;
            //         let msg: Msg<ProtocolMessage> = match serde_json::from_slice(&keygen_msg) {
            //             Ok(msg) => msg,
            //             Err(err) => {
            //                 error!(target: "dkg", "🕸️  Error deserializing msg: {:?}", err);
            //                 return Err(DKGError::GenericError {
            //                     reason: format!("Error deserializing keygen msg, reason: {}", err),
            //                 })
            //  gi           },
            //         };
            //     },
            //     // Offline stage
            //     DKGMsgPayload::Offline(msg) => todo!(),
            //     // Signing
            //     DKGMsgPayload::Vote(msg) => todo!(),
            //     DKGMsgPayload::PublicKeyBroadcast(_) => unimplemented!,
            //     DKGMsgPayload::MisbehaviourBroadcast(_) => unimplemented!,
            // }
            Msg { sender: party_index, receiver: None, body }
        })
    }
}

/// Check a `signature` of some `data` against a `set` of accounts.
/// Returns true if the signature was produced from an account in the set and false otherwise.
fn check_signers(data: &[u8], signature: &[u8], set: &[AccountId32]) -> bool {
    return dkg_runtime_primitives::utils::verify_signer_from_set(
        set.iter()
            .map(|x| {
                sr25519::Public(to_slice_32(&x.encode()).unwrap_or_else(|| {
                    panic!("Failed to convert account id to sr25519 public key")
                }))
            })
            .collect(),
        data,
        signature,
    )
    .1
}

/// Verifies a SignedDKGMessage was signed by the active or next authorities
fn verify_signature_against_authorities(
    signed_dkg_msg: SignedDKGMessage<Public>,
    active_authorities: &[AccountId32],
    next_authorities: &[AccountId32],
) -> Result<DKGMessage<Public>, DKGError> {
    let dkg_msg = signed_dkg_msg.msg;
    let encoded = dkg_msg.encode();
    let signature = signed_dkg_msg.signature.unwrap_or_default();
    
    if check_signers(&encoded, &signature, active_authorities) || check_signers(&encoded, &signature, next_authorities) {
        Ok(dkg_msg)
    } else {
        Err(DKGError::GenericError {
            reason: "Message signature is not from a registered authority or next authority"
                .into(),
        })
    }
}


pub trait TransformOutgoing {
    type Output;
    fn transform(self) -> Result<Self::Output, DKGError> where Self: Sized;
}

impl<T> Stream for IncomingAsyncProtocolWrapper<T>
where
    T: TransformIncoming,
{
    type Item = Result<Msg<T>, DKGError>;
    fn poll_next(
        mut self: Pin<&mut Self>, 
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        match futures::ready!(Pin::new(&mut self.receiver).poll_next(cx)) {
            Some(msg) => {
                match msg.transform() {
                    Ok(msg) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            },
            None => Poll::Ready(None),
        }
    }
}

impl<T> Sink<Msg<T>> for OutgoingAsyncProtocolWrapper<T>
where
    T: TransformOutgoing
{
    type Error = DKGError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_ready(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }

    fn start_send(mut self: Pin<&mut Self>, item: Msg<T>) -> Result<(), Self::Error> {
        let transformed_item = item.body.transform(); // result<T::Output>
        match transformed_item {
            Ok(item) => Pin::new(&mut self.sender).start_send(item).map_err(|err| DKGError::GenericError { reason: err.to_string() }),
            Err(err) => {
                // log the error
                error!(target: "dkg", "🕸️  Failed to transform outgoing message: {:?}", err);
                Ok(())
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_flush(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_close(cx)
            .map_err(|err| DKGError::GenericError { reason: err.to_string() })
    }
}