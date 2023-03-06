use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use bytes::Bytes;

type TransportFramed = Framed<tokio::net::TcpStream, LengthDelimitedCodec>;

pub fn bind_transport<T: AsyncRead + AsyncWrite>(io: T)
    -> Framed<T, LengthDelimitedCodec>
{
    Framed::new(io, LengthDelimitedCodec::new())
}

pub struct ReadHalf {
    inner: SplitStream<TransportFramed>
}

pub struct WriteHalf {
    inner: SplitSink<TransportFramed, Bytes>
}