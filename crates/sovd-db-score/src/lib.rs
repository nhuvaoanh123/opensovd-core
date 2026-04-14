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

//! S-CORE `score-persistency` stub backend for the [`SovdDb`] trait.
//!
//! This crate exists to **prove the trait seam fits** an S-CORE
//! key-value persistence store, per ADR-0016. It does not link
//! `score-persistency` (the upstream crate is not on the Taktflow dev
//! machine yet) — every method returns a well-defined
//! `NotYetImplemented` error so callers can distinguish "S-CORE backend
//! selected but not wired" from a real runtime failure.
//!
//! When Phase 4 wires the real backend, the only change is swapping the
//! method bodies; the trait surface stays the same.
//!
//! [`SovdDb`]: sovd_interfaces::traits::sovd_db::SovdDb

// TODO(phase-4): once `H:\taktflow-eclipsesdv-testing\score-score\` is
// checked out on the dev machine, add a path dependency on
// `score-persistency` behind the `score` feature and wire the methods
// below to its KVS API.

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    extras::fault::FaultRecord,
    spec::fault::{FaultDetails, FaultFilter, ListOfFaults},
    traits::sovd_db::{OperationCycleId, SovdDb},
    types::error::Result,
};

/// Stub [`SovdDb`] implementation that always returns
/// [`SovdError::Internal("not yet implemented")`].
#[derive(Debug, Default, Clone)]
pub struct ScoreSovdDb;

impl ScoreSovdDb {
    /// Construct a stub backend. In Phase 4 this becomes a real
    /// `score-persistency` handle constructor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn not_yet_implemented(method: &str) -> SovdError {
    SovdError::Internal(format!(
        "sovd-db-score::{method}: score-persistency backend not yet wired (Phase 4)"
    ))
}

#[async_trait]
impl SovdDb for ScoreSovdDb {
    async fn ingest_fault(&self, _record: FaultRecord) -> Result<()> {
        Err(not_yet_implemented("ingest_fault"))
    }

    async fn list_faults(&self, _filter: FaultFilter) -> Result<ListOfFaults> {
        Err(not_yet_implemented("list_faults"))
    }

    async fn get_fault(&self, _code: &str) -> Result<FaultDetails> {
        Err(not_yet_implemented("get_fault"))
    }

    async fn clear_faults(&self, _filter: FaultFilter) -> Result<()> {
        Err(not_yet_implemented("clear_faults"))
    }

    async fn clear_fault_by_code(&self, _code: &str) -> Result<()> {
        Err(not_yet_implemented("clear_fault_by_code"))
    }

    async fn snapshot_for_operation_cycle(&self, _cycle_id: &OperationCycleId) -> Result<()> {
        Err(not_yet_implemented("snapshot_for_operation_cycle"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_methods_report_not_yet_implemented() {
        let db = ScoreSovdDb::new();
        let err = db.list_faults(FaultFilter::all()).await.expect_err("stub");
        match err {
            SovdError::Internal(msg) => assert!(msg.contains("not yet wired")),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
