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

//! S-CORE `score-lifecycle` stub backend for the [`OperationCycle`] trait.
//!
//! Phase 3 deliverable per ADR-0016: proves the [`OperationCycle`] trait
//! seam fits an S-CORE lifecycle subscriber backend. Every method
//! returns `NotYetImplemented` until Phase 4 wires the real crate.
//!
//! [`OperationCycle`]: sovd_interfaces::traits::operation_cycle::OperationCycle

// TODO(phase-4): once `score-lifecycle` is checked out on the dev
// machine, add it as a path dependency behind the `score` feature and
// wire `start_cycle` / `end_cycle` to forward transitions into S-CORE's
// lifecycle manager, and `subscribe_events` to bridge from S-CORE
// lifecycle events into `tokio::sync::watch`.

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    traits::operation_cycle::{CurrentCycle, CycleName, OperationCycle, OperationCycleEvent},
    types::error::Result,
};
use tokio::sync::watch;

/// Stub [`OperationCycle`] implementation. Every call returns
/// [`SovdError::Internal`] with a "not yet wired" message.
#[derive(Debug)]
pub struct ScoreOperationCycle {
    // Hold a dummy channel so `subscribe_events` returns a valid type.
    tx: watch::Sender<OperationCycleEvent>,
}

impl Default for ScoreOperationCycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoreOperationCycle {
    /// Construct a stub driver. Phase 4 replaces this with a real
    /// `score-lifecycle` handle.
    #[must_use]
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(OperationCycleEvent::Idle);
        Self { tx }
    }
}

fn not_yet(method: &str) -> SovdError {
    SovdError::Internal(format!(
        "opcycle-score-lifecycle::{method}: score-lifecycle backend not yet wired (Phase 4)"
    ))
}

#[async_trait]
impl OperationCycle for ScoreOperationCycle {
    async fn current_cycle(&self) -> Result<CurrentCycle> {
        Err(not_yet("current_cycle"))
    }

    async fn start_cycle(&self, _name: CycleName) -> Result<()> {
        Err(not_yet("start_cycle"))
    }

    async fn end_cycle(&self, _name: CycleName) -> Result<()> {
        Err(not_yet("end_cycle"))
    }

    async fn subscribe_events(&self) -> watch::Receiver<OperationCycleEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_reports_not_yet_implemented() {
        let oc = ScoreOperationCycle::new();
        let err = oc.current_cycle().await.expect_err("stub");
        match err {
            SovdError::Internal(msg) => assert!(msg.contains("not yet wired")),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
