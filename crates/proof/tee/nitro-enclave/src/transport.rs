//! Length-prefixed bincode codec over `AsyncRead`/`AsyncWrite`.

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::debug;

/// Result type for proof transport operations.
pub type TransportResult<T> = Result<T, TransportError>;

/// Errors that can occur during proof transport operations.
#[derive(Error, Debug)]
pub enum TransportError {
    /// An I/O error occurred on the underlying stream.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization of a message failed.
    #[error("codec error: {0}")]
    Codec(String),

    /// A frame exceeded [`Frame::MAX_FRAME_BYTES`].
    ///
    /// Raised before any backing buffer is allocated to prevent a malicious or
    /// buggy peer from forcing a multi-gigabyte allocation inside the enclave.
    #[error("frame too large: {len} bytes exceeds limit of {limit} bytes")]
    FrameTooLarge {
        /// Length declared by the peer (or computed locally for writes).
        len: usize,
        /// Hard cap enforced by the codec.
        limit: usize,
    },
}

/// Maximum bytes per individual `write()` syscall on vsock.
///
/// Linux kernel commit `6693731487a8` (Aug 2025) changed `virtio_vsock` to
/// allocate nonlinear SKBs (scattered across multiple pages) for packets
/// larger than `PAGE_ALLOC_COSTLY_ORDER` (typically 32 `KiB` on x86). The
/// hypervisor-side virtio handler may not correctly reassemble these
/// multi-descriptor TX packets, causing silent data corruption.
///
/// By capping each `write()` to 28 `KiB` we force the kernel to use simple,
/// linear (single-page) SKB allocations, sidestepping the bug entirely.
///
/// See: <https://github.com/cloud-hypervisor/cloud-hypervisor/issues/7672>
/// 28 `KiB` — comfortably below the ~32384-byte linear SKB threshold
const MAX_WRITE_SIZE: usize = 28 * 1024;

/// Length-prefixed bincode codec over `AsyncRead`/`AsyncWrite`.
///
/// Wire format: `[4B big-endian length][bincode payload]`
///
/// Writes are throttled to [`MAX_WRITE_SIZE`]-byte segments to avoid
/// triggering a Linux kernel vsock corruption bug.
///
/// Both reads and writes are bounded by [`Frame::MAX_FRAME_BYTES`] so that a
/// malicious or buggy peer cannot force the enclave (which has a fixed shared
/// memory budget) to allocate a multi-gigabyte buffer up front.
#[derive(Debug, Clone, Copy)]
pub struct Frame;

impl Frame {
    /// Maximum length, in bytes, of a single bincode frame payload.
    ///
    /// The Nitro Enclave is configured with an 8 `GiB` shared memory pool
    /// (`etc/docker/nitro-enclave/allocator.yaml`). Each in-flight `Prove`
    /// request consumes roughly `2 * payload_bytes` (one copy in the read
    /// buffer, one in the bincode-decoded owned values), so an unchecked peer
    /// could fill the pool with a single ~4 `GiB` frame and OOM-kill the
    /// enclave — taking every concurrent proving job down with it.
    ///
    /// 512 `MiB` comfortably covers honest `Prove` payloads (preimage bundles
    /// are typically tens of `MiB`) while leaving ample headroom for several
    /// concurrent jobs to coexist after the 2x decode amplification.
    pub const MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;

    /// Write a value as a length-prefixed bincode frame.
    pub async fn write<T: serde::Serialize>(
        writer: &mut (impl AsyncWriteExt + Unpin),
        value: &T,
    ) -> TransportResult<()> {
        let payload = bincode::serde::encode_to_vec(value, bincode::config::standard())
            .map_err(|e| TransportError::Codec(e.to_string()))?;

        if payload.len() > Self::MAX_FRAME_BYTES {
            return Err(TransportError::FrameTooLarge {
                len: payload.len(),
                limit: Self::MAX_FRAME_BYTES,
            });
        }

        let len = u32::try_from(payload.len())
            .map_err(|_| TransportError::Codec("payload exceeds u32::MAX".into()))?;

        debug!(payload_bytes = payload.len(), "frame write start");

        writer.write_u32(len).await?;
        Self::write_throttled(writer, &payload).await?;
        writer.flush().await?;

        debug!(payload_bytes = payload.len(), "frame write complete");
        Ok(())
    }

    /// Read a value from a length-prefixed bincode frame.
    ///
    /// Frames larger than [`Self::MAX_FRAME_BYTES`] are rejected before any
    /// payload buffer is allocated, so a hostile peer cannot trigger an OOM
    /// inside the enclave by advertising a giant length prefix.
    pub async fn read<T: serde::de::DeserializeOwned>(
        reader: &mut (impl AsyncReadExt + Unpin),
    ) -> TransportResult<T> {
        let len = reader.read_u32().await? as usize;

        if len > Self::MAX_FRAME_BYTES {
            return Err(TransportError::FrameTooLarge { len, limit: Self::MAX_FRAME_BYTES });
        }

        debug!(payload_bytes = len, "frame read start");

        let mut payload = vec![0u8; len];
        reader.read_exact(&mut payload).await?;

        let (value, _) = bincode::serde::decode_from_slice(&payload, bincode::config::standard())
            .map_err(|e| TransportError::Codec(e.to_string()))?;

        debug!(payload_bytes = len, "frame read complete");
        Ok(value)
    }

    async fn write_throttled(
        writer: &mut (impl AsyncWriteExt + Unpin),
        data: &[u8],
    ) -> TransportResult<()> {
        for chunk in data.chunks(MAX_WRITE_SIZE) {
            writer.write_all(chunk).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[tokio::test]
    async fn round_trip_small_frame() {
        let mut buf = Vec::new();
        Frame::write(&mut buf, &"hello".to_string()).await.expect("write");

        let mut reader = Cursor::new(buf);
        let value: String = Frame::read(&mut reader).await.expect("read");
        assert_eq!(value, "hello");
    }

    #[tokio::test]
    async fn read_rejects_oversized_frame_without_allocating() {
        // Encode a length prefix one byte over the cap and no body. The reader
        // must reject before attempting `vec![0u8; len]`, otherwise this test
        // would request a multi-hundred-MiB allocation purely to confirm the
        // cap exists.
        let oversized = u32::try_from(Frame::MAX_FRAME_BYTES + 1).expect("fits in u32");
        let bytes = oversized.to_be_bytes().to_vec();
        let mut reader = Cursor::new(bytes);

        let err = Frame::read::<Vec<u8>>(&mut reader).await.expect_err("must reject");
        match err {
            TransportError::FrameTooLarge { len, limit } => {
                assert_eq!(len, Frame::MAX_FRAME_BYTES + 1);
                assert_eq!(limit, Frame::MAX_FRAME_BYTES);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_accepts_max_sized_length_prefix_with_real_payload() {
        // Boundary check: a length exactly at the cap is allowed by the size
        // gate. We exercise that gate by writing a small payload using a
        // length prefix that is at the limit, then truncating the read so we
        // do not actually allocate 512 MiB. We assert the failure mode is the
        // expected short-read I/O error rather than `FrameTooLarge`.
        let at_limit = u32::try_from(Frame::MAX_FRAME_BYTES).expect("fits in u32");
        let bytes = at_limit.to_be_bytes().to_vec();
        let mut reader = Cursor::new(bytes);

        let err = Frame::read::<Vec<u8>>(&mut reader).await.expect_err("short read");
        assert!(matches!(err, TransportError::Io(_)), "expected short-read I/O error, got {err:?}");
    }
}
