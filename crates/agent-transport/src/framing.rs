use agent_protocol::protocol::*;
use futures_util::StreamExt;
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};
pub async fn write(send: &mut iroh::endpoint::SendStream, value: impl Serialize) -> io::Result<()> {
    write_frame(send, &encode(value)?).await
}
/// Oversized results fail this request without closing unrelated streams.
pub async fn write_frame(send: &mut iroh::endpoint::SendStream, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds frame limit",
        ));
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(io::Error::other)?;
    send.write_all(bytes).await.map_err(io::Error::other)
}
/// Marks only a response's first bytes: per-poll records would evict the
/// connection setup from the bounded timeline within seconds.
struct ReadObservation {
    trace: std::sync::Arc<crate::diagnostics::connection::Trace>,
    group: u64,
    stream: u64,
    first: bool,
}

struct ObservedRecv {
    recv: iroh::endpoint::RecvStream,
    observation: Option<ReadObservation>,
}
impl tokio::io::AsyncRead for ObservedRecv {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = std::pin::Pin::new(&mut this.recv).poll_read(cx, buf);
        if let Some(observation) = this.observation.as_mut()
            && observation.first
            && buf.filled().len() > before
        {
            observation.first = false;
            observation.trace.record(
                crate::diagnostics::ConnectionPhase::ResponseFirstRead,
                observation.group,
                observation.stream,
                (buf.filled().len() - before) as u64,
            );
        }
        result
    }
}
pub struct Reader(FramedRead<ObservedRecv, LengthDelimitedCodec>);
impl Reader {
    pub fn new(recv: iroh::endpoint::RecvStream) -> Self {
        Self(
            LengthDelimitedCodec::builder()
                .max_frame_length(MAX_FRAME_BYTES)
                .new_read(ObservedRecv {
                    recv,
                    observation: None,
                }),
        )
    }
    pub(crate) fn observed(
        recv: iroh::endpoint::RecvStream,
        trace: std::sync::Arc<crate::diagnostics::connection::Trace>,
        group: u64,
    ) -> Self {
        let mut reader = Self::new(recv);
        let stream = reader.stream_id();
        reader.0.get_mut().observation = Some(ReadObservation {
            trace,
            group,
            stream,
            first: true,
        });
        reader
    }
    pub(crate) fn stream_id(&self) -> u64 {
        u64::from(self.0.get_ref().recv.id())
    }
    pub async fn read_frame(&mut self) -> io::Result<Option<tokio_util::bytes::BytesMut>> {
        let frame = self.0.next().await.transpose()?;
        if let Some(observation) = self.0.get_mut().observation.take() {
            observation.trace.record(
                crate::diagnostics::ConnectionPhase::ResponseReceived,
                observation.group,
                observation.stream,
                frame.as_ref().map_or(0, |bytes| bytes.len() as u64),
            );
        }
        Ok(frame)
    }
    /// FramedRead retains partial frames if this future loses a select.
    pub async fn read<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        let Some(bytes) = self.read_frame().await? else {
            return Ok(None);
        };
        decode(&bytes).map(Some)
    }
}
