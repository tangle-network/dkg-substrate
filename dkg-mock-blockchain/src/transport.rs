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
	Session { event: crate::MockBlockChainEvent },
	// Tells the client to halt the DKG and related networking services. The client is still
	// expected to listen to future commands such as "InitialHandshake" to restart the DKG once
	// the MockBlockchain determines it is time for the client to do so.
	Halt,
}

pub type TransportFramed = Framed<tokio::net::TcpStream, LengthDelimitedCodec>;

pub fn bind_transport<T: AsyncRead + AsyncWrite>(io: T) -> (WriteHalf, ReadHalf) {
	Framed::new(io, LengthDelimitedCodec::new()).split()
}

pub struct ReadHalf {
	inner: SplitStream<TransportFramed>,
}

pub struct WriteHalf {
	inner: SplitSink<TransportFramed, Bytes>,
}

impl Stream for ReadHalf {
	type Item = ProtocolPacket;
}

impl Sink<ProtocolPacket> for WriteHalf {}
