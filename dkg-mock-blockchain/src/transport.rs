use bytes::Bytes;
use futures::{
	stream::{SplitSink, SplitStream},
	Sink, Stream,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[derive(Serialize, Deserialize, Debug, Clone)]
/// A set of all the packets which will be exchanged between the client/server
/// after wrapping the TCP streams in the appropriate codecs via bind_transport
pub enum ProtocolPacket {
	// When a client first connects to the MockBlockchain, this packet gets sent
	InitialHandshake,
	// After the client receives the initial handshake, the client is expected to
	// return its peer id to the MockBlockchain
	InitialHandshakeResponse { peer_id: Vec<u8> },
	// After the handshake phase is complete, almost every packet sent back and forth
	// between the client and server uses this packet type
	BlockChainToClient { event: crate::MockBlockChainEvent },
	ClientToBlockChain { event: crate::MockClientResponse },
	// Tells the client to halt the DKG and related networking services.
	Halt,
}

pub type TransportFramed = Framed<tokio::net::TcpStream, LengthDelimitedCodec>;

pub fn bind_transport<T: AsyncRead + AsyncWrite>(io: T) -> (WriteHalf, ReadHalf) {
	let (tx, rx) = Framed::new(io, LengthDelimitedCodec::new()).split();
	(WriteHalf { inner: tx }, ReadHalf { inner: rx })
}

pub struct ReadHalf {
	inner: SplitStream<TransportFramed>,
}

pub struct WriteHalf {
	inner: SplitSink<TransportFramed, Bytes>,
}

impl Stream for ReadHalf {
	type Item = ProtocolPacket;
	fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
		if let Some(bytes) = futures::ready!(Pin::new(&mut self.get_mut().inner).poll_next(cx)) {
			// convert bytes to a ProtocolPacket
			if let Ok(packet) = bincode2::deserialize(&bytes[..]) {
				Poll::Ready(Some(packet))
			} else {
				panic!("Received an invalid protocol packet")
			}
		} else {
			// stream died
			std::task::Poll::Ready(None)
		}
	}
}

impl Sink<ProtocolPacket> for WriteHalf {
	fn poll_close(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.get_mut().inner).poll_close(cx)
	}
	fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.get_mut().inner).poll_flush(cx)
	}
	fn poll_ready(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.get_mut().inner).poll_ready(cx)
	}
	fn start_send(self: std::pin::Pin<&mut Self>, item: ProtocolPacket) -> Result<(), Self::Error> {
		let bytes = bincode2::serialize(&item).unwrap();
		Pin::new(&mut self.get_mut().inner).start_send(Bytes::from(bytes))
	}
}
