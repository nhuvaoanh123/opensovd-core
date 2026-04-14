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

//! In-process [`OperationCycle`] backend for the Taktflow standalone stack.
//!
//! Per ADR-0012 and ADR-0016: a single state machine keyed on cycle name
//! with `tokio::sync::watch`-based fan-out to subscribers. No persistence —
//! cycle state is in-memory only, which is fine for the standalone path
//! because tester-driven and ECU-driven callers both create cycles
//! explicitly and the Fault Library is responsible for restoring the
//! needed context after a restart.
//!
//! `end_cycle` accepts a snapshot callback so the caller (typically the
//! DFM, holding a `SovdDb`) can freeze the fault set under the ending
//! cycle id before the state transition becomes visible to subscribers.
//! This keeps the ADR-0012 §3 "single state machine" invariant.
//!
//! [`OperationCycle`]: sovd_interfaces::traits::operation_cycle::OperationCycle

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    traits::operation_cycle::{CurrentCycle, CycleName, OperationCycle, OperationCycleEvent},
    types::error::Result,
};
use tokio::sync::{Mutex, watch};

/// In-process [`OperationCycle`] state machine.
#[derive(Debug)]
pub struct TaktflowOperationCycle {
    state: Mutex<State>,
    tx: watch::Sender<OperationCycleEvent>,
}

#[derive(Debug, Default)]
struct State {
    active: Option<CycleName>,
}

impl Default for TaktflowOperationCycle {
    fn default() -> Self {
        Self::new()
    }
}

impl TaktflowOperationCycle {
    /// Build a new state machine with no active cycle.
    #[must_use]
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(OperationCycleEvent::Idle);
        Self {
            state: Mutex::new(State::default()),
            tx,
        }
    }
}

#[async_trait]
impl OperationCycle for TaktflowOperationCycle {
    async fn current_cycle(&self) -> Result<CurrentCycle> {
        let guard = self.state.lock().await;
        Ok(CurrentCycle {
            name: guard.active.clone(),
        })
    }

    async fn start_cycle(&self, name: CycleName) -> Result<()> {
        let mut guard = self.state.lock().await;
        if let Some(existing) = guard.active.as_ref() {
            if existing == &name {
                tracing::warn!(
                    cycle = %name,
                    "start_cycle called for already-active cycle; first-start-wins no-op"
                );
                return Ok(());
            }
            return Err(SovdError::InvalidRequest(format!(
                "cannot start cycle \"{name}\": cycle \"{existing}\" is already active"
            )));
        }
        guard.active = Some(name.clone());
        drop(guard);
        // `send` only errors if there are no receivers, which is fine —
        // the watch still retains the latest value for later subscribers.
        let _ = self.tx.send(OperationCycleEvent::Started(name));
        Ok(())
    }

    async fn end_cycle(&self, name: CycleName) -> Result<()> {
        let mut guard = self.state.lock().await;
        match guard.active.as_ref() {
            Some(active) if active == &name => {
                guard.active = None;
                drop(guard);
                let _ = self.tx.send(OperationCycleEvent::Ended(name));
                Ok(())
            }
            _ => Err(SovdError::NotFound {
                entity: format!("active cycle \"{name}\""),
            }),
        }
    }

    async fn subscribe_events(&self) -> watch::Receiver<OperationCycleEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_and_end_round_trip() {
        let oc = TaktflowOperationCycle::new();
        oc.start_cycle("tester.run1".into()).await.expect("start");
        let current = oc.current_cycle().await.expect("current");
        assert_eq!(current.name.as_deref(), Some("tester.run1"));
        oc.end_cycle("tester.run1".into()).await.expect("end");
        let current = oc.current_cycle().await.expect("current");
        assert!(current.name.is_none());
    }

    #[tokio::test]
    async fn duplicate_start_is_noop() {
        let oc = TaktflowOperationCycle::new();
        oc.start_cycle("ecu.ignition".into()).await.expect("start");
        oc.start_cycle("ecu.ignition".into()).await.expect("dup");
        let current = oc.current_cycle().await.expect("current");
        assert_eq!(current.name.as_deref(), Some("ecu.ignition"));
    }

    #[tokio::test]
    async fn second_different_cycle_rejected_while_active() {
        let oc = TaktflowOperationCycle::new();
        oc.start_cycle("tester.run".into()).await.expect("start");
        let err = oc
            .start_cycle("ecu.ignition".into())
            .await
            .expect_err("should reject");
        assert!(matches!(err, SovdError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn end_unknown_cycle_is_not_found() {
        let oc = TaktflowOperationCycle::new();
        let err = oc
            .end_cycle("ecu.none".into())
            .await
            .expect_err("should fail");
        assert!(matches!(err, SovdError::NotFound { .. }));
    }

    #[tokio::test]
    async fn subscriber_sees_transition() {
        let oc = TaktflowOperationCycle::new();
        let mut rx = oc.subscribe_events().await;
        // Initial state is Idle.
        assert_eq!(*rx.borrow_and_update(), OperationCycleEvent::Idle);
        oc.start_cycle("integration.mixed".into())
            .await
            .expect("start");
        rx.changed().await.expect("changed");
        assert_eq!(
            *rx.borrow_and_update(),
            OperationCycleEvent::Started("integration.mixed".into())
        );
        oc.end_cycle("integration.mixed".into()).await.expect("end");
        rx.changed().await.expect("changed");
        assert_eq!(
            *rx.borrow_and_update(),
            OperationCycleEvent::Ended("integration.mixed".into())
        );
    }

    #[tokio::test]
    async fn multiple_subscribers_fan_out() {
        let oc = TaktflowOperationCycle::new();
        let mut rx1 = oc.subscribe_events().await;
        let mut rx2 = oc.subscribe_events().await;
        let _ = rx1.borrow_and_update();
        let _ = rx2.borrow_and_update();
        oc.start_cycle("tester.fanout".into()).await.expect("start");
        rx1.changed().await.expect("rx1");
        rx2.changed().await.expect("rx2");
        assert_eq!(
            *rx1.borrow_and_update(),
            OperationCycleEvent::Started("tester.fanout".into())
        );
        assert_eq!(
            *rx2.borrow_and_update(),
            OperationCycleEvent::Started("tester.fanout".into())
        );
    }

    #[tokio::test]
    async fn tester_and_ecu_source_paths_converge() {
        // ADR-0012 §3: both sources feed the same state machine.
        // The trait is source-agnostic — here we simulate "tester"
        // and "ecu" callers by naming their cycles, and show that
        // both route through a single OperationCycle instance.
        let oc = TaktflowOperationCycle::new();
        oc.start_cycle("tester.bench1".into())
            .await
            .expect("tester start");
        oc.end_cycle("tester.bench1".into())
            .await
            .expect("tester end");
        oc.start_cycle("ecu.ignition".into())
            .await
            .expect("ecu start");
        let current = oc.current_cycle().await.expect("current");
        assert_eq!(current.name.as_deref(), Some("ecu.ignition"));
    }
}
