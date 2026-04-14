/*
 * SPDX-License-Identifier: Apache-2.0
 * SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Apache License Version 2.0 which is available at
 * https://www.apache.org/licenses/LICENSE-2.0
 */

//! Length-prefixed postcard codec shared by both transports.

use sovd_interfaces::{SovdError, extras::fault::FaultRecord, types::error::Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Upper bound for a single encoded `FaultRecord`. Guards against a
/// malicious or corrupted sender sending a huge length prefix that would
/// otherwise eat memory.
pub const MAX_FRAME_LEN: usize = 64 * 1024;

/// Encode a [`FaultRecord`] into the on-wire framed bytes.
///
/// # Errors
///
/// Returns [`SovdError::Internal`] if postcard encoding fails or the
/// frame exceeds [`MAX_FRAME_LEN`].
pub fn encode_frame(record: &FaultRecord) -> Result<Vec<u8>> {
    let payload = postcard::to_allocvec(record)
        .map_err(|e| SovdError::Internal(format!("postcard encode failed: {e}")))?;
    if payload.len() > MAX_FRAME_LEN {
        return Err(SovdError::InvalidRequest(format!(
            "encoded fault record exceeds {MAX_FRAME_LEN} bytes ({} actual)",
            payload.len()
        )));
    }
    let len_u32 = u32::try_from(payload.len()).map_err(|_| {
        SovdError::Internal("encoded fault record length does not fit u32".to_owned())
    })?;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len_u32.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Write a single framed [`FaultRecord`] to `writer`.
///
/// # Errors
///
/// Returns [`SovdError::Transport`] for I/O errors,
/// [`SovdError::Internal`] for encoding errors.
pub async fn write_frame<W: AsyncWriteExt + Unpin>(writer: &mut W, record: &FaultRecord) -> Result<()> {
    let frame = encode_frame(record)?;
    writer
        .write_all(&frame)
        .await
        .map_err(|e| SovdError::Transport(format!("fault-sink write: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| SovdError::Transport(format!("fault-sink flush: {e}")))?;
    Ok(())
}

/// Read a single framed [`FaultRecord`] from `reader`.
///
/// Returns `Ok(None)` on clean EOF before any bytes of the length prefix.
///
/// # Errors
///
/// Returns [`SovdError::Transport`] for I/O errors,
/// [`SovdError::InvalidRequest`] for oversized frames, and
/// [`SovdError::Internal`] for decode errors.
pub async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Option<FaultRecord>> {
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(SovdError::Transport(format!("fault-sink read len: {e}"))),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(SovdError::InvalidRequest(format!(
            "frame length {len} exceeds MAX_FRAME_LEN={MAX_FRAME_LEN}"
        )));
    }
    let mut payload = vec![0_u8; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|e| SovdError::Transport(format!("fault-sink read payload: {e}")))?;
    let record: FaultRecord = postcard::from_bytes(&payload)
        .map_err(|e| SovdError::Internal(format!("postcard decode failed: {e}")))?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovd_interfaces::{
        ComponentId,
        extras::fault::{FaultId, FaultRecord, FaultSeverity},
    };

    fn sample() -> FaultRecord {
        FaultRecord {
            component: ComponentId::new("cvc"),
            id: FaultId(0x00_01_02),
            severity: FaultSeverity::Error,
            timestamp_ms: 4_200,
            meta: Some(serde_json::json!({"k": "v"})),
        }
    }

    #[tokio::test]
    async fn frame_roundtrip_in_memory() {
        let r = sample();
        let bytes = encode_frame(&r).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes);
        let back = read_frame(&mut cursor).await.expect("read").expect("some");
        assert_eq!(back, r);
    }

    #[tokio::test]
    async fn empty_reader_returns_none() {
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        let out = read_frame(&mut cursor).await.expect("read");
        assert!(out.is_none());
    }
}
