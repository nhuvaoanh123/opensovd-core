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

#![allow(clippy::doc_markdown)]
// ADR-0018 D7: deny expect_used in production backend code. Tests
// keep expect() for readability — propagating errors through the
// integration test runner provides no extra diagnostic value when
// the assertion machinery already captures panics.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! Unix-socket [`FaultSink`] backend.
//!
//! Default standalone Fault Library IPC path per ADR-0002 and ADR-0016.
//!
//! # Platform note — unix sockets on Windows
//!
//! Windows 10 1803+ supports `AF_UNIX` natively, but Tokio's
//! `UnixListener` / `UnixStream` types are only exposed on `cfg(unix)`.
//! On Windows we use `tokio::net::windows::named_pipe` with the same
//! length-prefixed wire format, so the DFM binary has a usable IPC path
//! on both dev platforms. The two transports share the `codec` module —
//! wire format is identical.
//!
//! # Wire format
//!
//! ```text
//! [4 bytes LE u32 length] [postcard-encoded FaultRecord]
//! ```
//!
//! **Why postcard, not bincode?** postcard is no_std-friendly (the
//! embedded Fault Library shim in a later phase may want to encode from
//! an RTOS without pulling std), has a stable wire format, and is
//! explicitly documented as a cross-language target. bincode 1.x has a
//! looser wire contract. See ADR-0016 for the broader pluggability
//! rationale.
//!
//! [`FaultSink`]: sovd_interfaces::traits::fault_sink::FaultSink

pub mod codec;

#[cfg(unix)]
mod transport_unix;
#[cfg(windows)]
mod transport_windows;

#[cfg(unix)]
pub use transport_unix::{UnixFaultSink, UnixFaultSource};
#[cfg(windows)]
pub use transport_windows::{
    NamedPipeFaultSink as UnixFaultSink, NamedPipeFaultSource as UnixFaultSource,
};
