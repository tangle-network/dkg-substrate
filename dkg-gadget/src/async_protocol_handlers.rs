use std::{
	pin::Pin,
	task::{Context, Poll},
};

use dkg_primitives::types::DKGError;
use futures::{stream::Stream, Sink};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Keygen;
use round_based::{async_runtime::AsyncProtocol, Msg, StateMachine};

// let (tx, rx) = futures::channel() // incoming
// let (tx1, rx2) = futures::channel() // outgoing
// pub fn new(state: SM, incoming: I, outgoing: O) -> Self
// AsyncProtocol::new(state_machine, IncomingAsyncProtocolWrapper { receiver: rx },
// OutgoingAsyncProtocolWrapper {. })

pub struct IncomingAsyncProtocolWrapper<T> {
	pub receiver: futures::channel::mpsc::UnboundedReceiver<T>,
}

pub struct OutgoingAsyncProtocolWrapper<T> {
	pub sender: futures::channel::mpsc::UnboundedSender<T>,
}

pub trait TransformIncoming {
	fn transform(self) -> Result<Msg<Self>, DKGError>
	where
		Self: Sized;
}

pub trait TransformOutgoing {
	fn transform(self) -> Result<Self, DKGError>
	where
		Self: Sized;
}

impl<T> Stream for IncomingAsyncProtocolWrapper<T>
where
	T: TransformIncoming,
{
	type Item = Result<Msg<T>, DKGError>;
	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		match futures::ready!(Pin::new(&mut self.receiver).poll_next(cx)) {
			Some(msg) => match msg.transform() {
				Ok(msg) => Poll::Ready(Some(Ok(msg))),
				Err(e) => Poll::Ready(Some(Err(e))),
			},
			None => Poll::Ready(None),
		}
	}
}

impl<T> Sink<Msg<T>> for OutgoingAsyncProtocolWrapper<T>
where
	T: TransformOutgoing,
{
	type Error = DKGError;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		todo!()
	}

	fn start_send(self: Pin<&mut Self>, item: Msg<T>) -> Result<(), Self::Error> {
		todo!()
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		todo!()
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		todo!()
	}
}
