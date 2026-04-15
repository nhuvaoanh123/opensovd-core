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

//! Windows named-pipe transport for the Fault Library IPC path.
//!
//! Tokio exposes `UnixListener` / `UnixStream` only on `cfg(unix)`. On
//! Windows dev we use `tokio::net::windows::named_pipe`, which carries
//! the same length-prefixed postcard frames. The wire format is
//! identical — only the transport endpoint differs. See the crate-level
//! docs on `fault_sink_unix` for the rationale.

use std::ffi::{OsStr, OsString};

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    extras::fault::FaultRecord,
    traits::fault_sink::{FaultRecordRef, FaultSink},
    types::error::Result,
};
use tokio::{
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
    sync::Mutex,
};

use crate::codec;

/// Named-pipe client half (the Fault Library shim side). Exposed under
/// the `UnixFaultSink` alias from the crate root so code that names the
/// sink type works unchanged across platforms.
pub struct NamedPipeFaultSink {
    stream: Mutex<NamedPipeClient>,
}

impl NamedPipeFaultSink {
    /// Connect to a named pipe at `path`.
    ///
    /// `path` must be a full Windows pipe path like
    /// `\\.\pipe\opensovd-fault-sink`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] if the connect fails.
    //
    // `async` for parity with the Unix `connect` signature — callers
    // are written cross-platform and `.await` either implementation.
    #[allow(clippy::unused_async)]
    pub async fn connect(path: &OsStr) -> Result<Self> {
        let client = ClientOptions::new()
            .open(path)
            .map_err(|e| SovdError::Transport(format!("named-pipe connect failed: {e}")))?;
        Ok(Self {
            stream: Mutex::new(client),
        })
    }
}

#[async_trait]
impl FaultSink for NamedPipeFaultSink {
    async fn record_fault<'buf>(&self, record: FaultRecordRef<'buf>) -> Result<()> {
        let owned: FaultRecord = record.into_owned();
        let mut guard = self.stream.lock().await;
        codec::write_frame(&mut *guard, &owned).await
    }
}

/// Named-pipe server half (the DFM side).
pub struct NamedPipeFaultSource {
    server: Mutex<Option<NamedPipeServer>>,
    path: OsString,
}

impl NamedPipeFaultSource {
    /// Bind a named pipe at `path`. The first connection will consume
    /// this server; a fresh one must be created for subsequent
    /// connections (matches the stdlib pattern).
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] if the bind fails.
    pub fn bind(path: &OsStr) -> Result<Self> {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(path)
            .map_err(|e| SovdError::Transport(format!("named-pipe bind failed: {e}")))?;
        Ok(Self {
            server: Mutex::new(Some(server)),
            path: path.to_owned(),
        })
    }

    /// Accept one connection and read framed records until EOF.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Transport`] if accept fails,
    /// [`SovdError::Internal`] if called twice without a re-bind.
    pub async fn accept_and_drain<F>(&self, mut on_record: F) -> Result<()>
    where
        F: FnMut(FaultRecord) -> Result<()> + Send,
    {
        let mut guard = self.server.lock().await;
        let Some(mut server) = guard.take() else {
            return Err(SovdError::Internal(
                "named-pipe source already consumed; rebind to accept another".to_owned(),
            ));
        };
        server
            .connect()
            .await
            .map_err(|e| SovdError::Transport(format!("named-pipe connect wait: {e}")))?;
        drop(guard);
        while let Some(record) = codec::read_frame(&mut server).await? {
            on_record(record)?;
        }
        Ok(())
    }

    /// Path this source is bound to.
    #[must_use]
    pub fn path(&self) -> &OsStr {
        &self.path
    }
}
