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

//! S-CORE `score-communication` (LoLa) stub backend for the
//! [`FaultSink`] trait.
//!
//! Phase 3 deliverable per ADR-0016: proves the widened
//! [`FaultRecordRef`] buffer-lifetime contract fits a zero-copy
//! shared-memory LoLa skeleton/proxy backend. Every method returns
//! `NotYetImplemented` until Phase 4 wires the real crate.
//!
//! [`FaultSink`]: sovd_interfaces::traits::fault_sink::FaultSink
//! [`FaultRecordRef`]: sovd_interfaces::traits::fault_sink::FaultRecordRef

// TODO(phase-4): once `score-communication` is checked out on the dev
// machine, add it as a path dependency behind the `score` feature and
// wire `record_fault` to publish the record into the appropriate LoLa
// event slot using the `FaultRecordRef::Borrowed` variant to avoid
// copying out of the shared-memory arena.

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    traits::fault_sink::{FaultRecordRef, FaultSink},
    types::error::Result,
};

/// Stub [`FaultSink`] implementation. Every call returns
/// [`SovdError::Internal`] with a "not yet wired" message.
#[derive(Debug, Default, Clone)]
pub struct LolaFaultSink;

impl LolaFaultSink {
    /// Construct a stub. Phase 4 replaces this with a real LoLa handle
    /// constructor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FaultSink for LolaFaultSink {
    async fn record_fault<'buf>(&self, _record: FaultRecordRef<'buf>) -> Result<()> {
        Err(SovdError::Internal(
            "fault-sink-lola::record_fault: score-communication backend not yet wired (Phase 4)"
                .to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use sovd_interfaces::{
        ComponentId,
        extras::fault::{FaultId, FaultRecord, FaultSeverity},
    };

    use super::*;

    #[tokio::test]
    async fn stub_reports_not_yet_implemented() {
        let sink = LolaFaultSink::new();
        let record = FaultRecord {
            component: ComponentId::new("cvc"),
            id: FaultId(0x01),
            severity: FaultSeverity::Info,
            timestamp_ms: 0,
            meta: None,
        };
        let err = sink.record_fault(record.into()).await.expect_err("stub");
        match err {
            SovdError::Internal(msg) => assert!(msg.contains("not yet wired")),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
