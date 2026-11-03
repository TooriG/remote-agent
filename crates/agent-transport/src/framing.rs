use agent_protocol::protocol::*;
use futures_util::StreamExt;
use serde::{Serialize, de::DeserializeOwned};
use std::{borrow::Cow, io};
use tokio_util::bytes::{Bytes, BytesMut};
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

// Wire format: u32 length, u8 encoding, payload. The length includes the encoding
// byte; the existing limit applies to the MessagePack bytes before compression.
const RAW: u8 = 0;
const ZSTD: u8 = 1;
const COMPRESSION_THRESHOLD: usize = 4096;

pub async fn write(send: &mut iroh::endpoint::SendStream, value: impl Serialize) -> io::Result<()> {
    write_frame(send, &encode(value)?).await
}
/// Oversized results fail this request without closing unrelated streams.
pub async fn write_frame(send: &mut iroh::endpoint::SendStream, bytes: &[u8]) -> io::Result<()> {
    let (encoding, payload) = encode_payload(bytes)?;
    let mut header = [0; 5];
    header[..4].copy_from_slice(&((payload.len() + 1) as u32).to_be_bytes());
    header[4] = encoding;
    send.write_all(&header).await.map_err(io::Error::other)?;
    send.write_all(&payload).await.map_err(io::Error::other)
}

fn encode_payload(bytes: &[u8]) -> io::Result<(u8, Cow<'_, [u8]>)> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds frame limit",
        ));
    }
    // Independent level-1 frames keep small updates immediate and avoid shared
    // compression state across requests. Images and other incompressible input
    // retain their original bytes when compression would increase wire size.
    if bytes.len() >= COMPRESSION_THRESHOLD {
        let compressed = zstd::bulk::compress(bytes, 1)?;
        if compressed.len() < bytes.len() {
            return Ok((ZSTD, Cow::Owned(compressed)));
        }
    }
    Ok((RAW, Cow::Borrowed(bytes)))
}

fn decode_payload(mut frame: BytesMut) -> io::Result<Bytes> {
    if frame.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing frame encoding",
        ));
    }
    match frame.split_to(1)[0] {
        RAW => Ok(frame.freeze()),
        ZSTD => {
            // Exactly one Zstd frame is allowed. Bulk decompression bounds the
            // output even if the frame omits or lies about its content size.
            if zstd::zstd_safe::find_frame_compressed_size(&frame).map_err(|code| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    zstd::zstd_safe::get_error_name(code),
                )
            })? != frame.len()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "trailing compressed data",
                ));
            }
            zstd::bulk::decompress(&frame, MAX_FRAME_BYTES).map(Bytes::from)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown frame encoding",
        )),
    }
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
                .max_frame_length(MAX_FRAME_BYTES + 1)
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
    pub async fn read_frame(&mut self) -> io::Result<Option<Bytes>> {
        let frame = self.0.next().await.transpose()?;
        if let Some(observation) = self.0.get_mut().observation.take() {
            observation.trace.record(
                crate::diagnostics::ConnectionPhase::ResponseReceived,
                observation.group,
                observation.stream,
                frame.as_ref().map_or(0, |bytes| bytes.len() as u64),
            );
        }
        frame.map(decode_payload).transpose()
    }
    /// FramedRead retains partial frames if this future loses a select.
    pub async fn read<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        let Some(bytes) = self.read_frame().await? else {
            return Ok(None);
        };
        decode(&bytes).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn wire_payload(encoding: u8, payload: &[u8]) -> BytesMut {
        let mut frame = BytesMut::with_capacity(payload.len() + 1);
        frame.extend_from_slice(&[encoding]);
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn small_updates_stay_raw_and_large_text_is_compressed() {
        let small = vec![b'x'; 4095];
        let (encoding, payload) = encode_payload(&small).unwrap();
        assert_eq!(encoding, RAW);
        assert!(matches!(payload, Cow::Borrowed(_)));
        assert_eq!(payload.as_ref(), small);

        let text = vec![b'x'; 4096];
        let (encoding, payload) = encode_payload(&text).unwrap();
        assert_eq!(encoding, ZSTD);
        assert!(payload.len() < text.len());
        assert_eq!(
            decode_payload(wire_payload(encoding, &payload)).unwrap(),
            text
        );
    }

    #[test]
    fn incompressible_data_stays_raw() {
        let mut bytes = vec![0; 65536];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).unwrap();
        let (encoding, payload) = encode_payload(&bytes).unwrap();
        assert_eq!(encoding, RAW);
        assert!(matches!(payload, Cow::Borrowed(_)));
        assert_eq!(payload.as_ref(), bytes);
    }

    #[test]
    fn enforces_the_uncompressed_limit_on_send_and_receive() {
        let mut bytes = vec![b'x'; MAX_FRAME_BYTES];
        let (encoding, payload) = encode_payload(&bytes).unwrap();
        assert_eq!(
            decode_payload(wire_payload(encoding, &payload)).unwrap(),
            bytes
        );
        bytes.push(b'x');
        assert!(encode_payload(&bytes).is_err());
        let compressed = zstd::bulk::compress(&bytes, 1).unwrap();
        assert!(decode_payload(wire_payload(ZSTD, &compressed)).is_err());

        // The same bound applies when no content size was written in the header.
        let mut compressor = zstd::bulk::Compressor::new(1).unwrap();
        compressor.include_contentsize(false).unwrap();
        let compressed = compressor.compress(&bytes).unwrap();
        assert!(decode_payload(wire_payload(ZSTD, &compressed)).is_err());
    }

    #[test]
    fn rejects_missing_unknown_and_invalid_encodings() {
        for frame in [
            BytesMut::new(),
            wire_payload(2, b"text"),
            wire_payload(ZSTD, b"not a Zstd frame"),
            wire_payload(ZSTD, &[]),
        ] {
            assert!(decode_payload(frame).is_err());
        }
    }

    #[test]
    fn rejects_truncated_trailing_and_concatenated_compressed_data() {
        let compressed = zstd::bulk::compress(b"a complete message", 1).unwrap();
        for end in 0..compressed.len() {
            assert!(decode_payload(wire_payload(ZSTD, &compressed[..end])).is_err());
        }
        let mut trailing = compressed.clone();
        trailing.push(0);
        assert!(decode_payload(wire_payload(ZSTD, &trailing)).is_err());
        let mut concatenated = compressed.clone();
        concatenated.extend_from_slice(&compressed);
        assert!(decode_payload(wire_payload(ZSTD, &concatenated)).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_messages_round_trip(bytes in prop_oneof![
            proptest::collection::vec(any::<u8>(), 0..32768),
            (any::<u8>(), 0usize..65536).prop_map(|(byte, length)| vec![byte; length]),
        ]) {
            let (encoding, payload) = encode_payload(&bytes).unwrap();
            prop_assert!(payload.len() <= bytes.len());
            let decoded = decode_payload(wire_payload(encoding, &payload)).unwrap();
            prop_assert_eq!(decoded.as_ref(), bytes);
        }
    }
}
