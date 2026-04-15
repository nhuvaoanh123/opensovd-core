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

//! POSIX `tokio::net::UnixStream` transport for the Fault Library IPC
//! path. Used on Linux (Pi gateway) and macOS (dev).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    extras::fault::FaultRecord,
    traits::fault_sink::{FaultRecordRef, FaultSink},
    types::error::Result,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

use crate::codec;

/// `FaultSink` that writes each record to a shared `UnixStream`
/// connected to a server at `path`.
pub struct UnixFaultSink {
    stream: Mutex<UnixStream>,
}

impl UnixFaultSink {
    /// Connect to a Unix-socket listener at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] if the connect fails.
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| SovdError::Transport(format!("unix connect failed: {e}")))?;
        Ok(Self {
            stream: Mutex::new(stream),
        })
    }
}

#[async_trait]
impl FaultSink for UnixFaultSink {
    async fn record_fault<'buf>(&self, record: FaultRecordRef<'buf>) -> Result<()> {
        let owned: FaultRecord = record.into_owned();
        let mut guard = self.stream.lock().await;
        codec::write_frame(&mut *guard, &owned).await
    }
}

/// Server-side half: accepts connections and decodes framed
/// [`FaultRecord`]s into an `mpsc` channel the DFM task can consume.
pub struct UnixFaultSource {
    listener: UnixListener,
    path: PathBuf,
}

impl UnixFaultSource {
    /// Bind a listener at `path`. Removes any existing stale socket first.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] if the bind fails.
    pub fn bind(path: &Path) -> Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)
            .map_err(|e| SovdError::Transport(format!("unix bind failed: {e}")))?;
        Ok(Self {
            listener,
            path: path.to_owned(),
        })
    }

    /// Accept one connection and read framed records until EOF, pushing
    /// each into `on_record`. Returns after the client disconnects.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] for I/O errors on accept.
    pub async fn accept_and_drain<F>(&self, mut on_record: F) -> Result<()>
    where
        F: FnMut(FaultRecord) -> Result<()> + Send,
    {
        let (mut stream, _addr) = self
            .listener
            .accept()
            .await
            .map_err(|e| SovdError::Transport(format!("unix accept failed: {e}")))?;
        while let Some(record) = codec::read_frame(&mut stream).await? {
            on_record(record)?;
        }
        Ok(())
    }

    /// Path this source is bound to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UnixFaultSource {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
