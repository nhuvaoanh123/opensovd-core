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

//! [`OperationCycle`] — DFM lifecycle driver trait.
//!
//! The DFM treats faults as active, debounced, or suppressed depending on
//! the current **operation cycle** — the named lifecycle window during
//! which a given set of fault events is considered. A typical automotive
//! operation cycle starts on ignition-on and ends on ignition-off.
//!
//! Per ADR-0012 ("DFM Operation-Cycle API — Support Both Tester-Driven and
//! ECU-Driven"), cycles may be started from two sources:
//!
//! 1. **Tester-driven**, via the REST entry point
//!    `POST /sovd/v1/operation-cycles/{cycle_name}/start|end`
//! 2. **ECU-driven**, via the Fault Library shim's
//!    `FaultShim_OperationCycleStart()` IPC call
//!
//! Both sources converge on a single state machine that implements this
//! trait. The trait itself is source-agnostic — the caller decides who
//! drives it.
//!
//! Per ADR-0016, this trait has two anticipated backends:
//!
//! - `opcycle-taktflow` — default standalone backend, an in-process state
//!   machine with `tokio::sync::watch`-based subscribers.
//! - `opcycle-score-lifecycle` — optional S-CORE backend that subscribes
//!   to `score-lifecycle` events and maps them onto cycle edges.

use async_trait::async_trait;
use tokio::sync::watch;

use crate::types::error::Result;

/// Free-form name of an operation cycle. Per ADR-0012 §"Cycle name
/// namespace", the convention is `tester.*`, `ecu.*`, or `integration.*`
/// prefixes, but the trait does not enforce this.
pub type CycleName = String;

/// Event emitted by an [`OperationCycle`] state machine.
///
/// `PartialEq` + `Eq` + `Clone` so subscribers can easily compare and fan
/// out. Not `Copy` because `CycleName` is a `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationCycleEvent {
    /// A new cycle started. Payload is the cycle name.
    Started(CycleName),
    /// A cycle ended. Payload is the cycle name.
    Ended(CycleName),
    /// Initial state on subscription — no cycle is currently active.
    /// Subscribers receive this on the initial read so they can
    /// distinguish "just subscribed" from "cycle just ended".
    Idle,
}

/// Snapshot of the current cycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentCycle {
    /// Name of the currently active cycle, or `None` if no cycle is active.
    pub name: Option<CycleName>,
}

/// Operation-cycle lifecycle driver.
///
/// Both tester-driven and ECU-driven callers route through the same
/// implementation per ADR-0012 §3 ("single internal state machine").
/// Backends MUST enforce the "first start wins" rule from ADR-0012 §5 —
/// a duplicate `start_cycle` for an already-active cycle is a no-op with
/// a warning log, not an error.
#[async_trait]
pub trait OperationCycle: Send + Sync {
    /// Return the currently active cycle, if any.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// if the backing state cannot be read.
    async fn current_cycle(&self) -> Result<CurrentCycle>;

    /// Start a cycle with the given name.
    ///
    /// Per ADR-0012 §5 "first start wins": if a cycle with the same name
    /// is already active, this is a no-op with a warning logged by the
    /// implementation. Callers cannot observe the difference through the
    /// return value.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// if the state machine is wedged.
    async fn start_cycle(&self, name: CycleName) -> Result<()>;

    /// End the currently active cycle named `name`.
    ///
    /// The implementation should snapshot the DFM's fault set against the
    /// ending cycle id before the state transition becomes visible to
    /// subscribers, so consumers of [`Self::subscribe_events`] always see
    /// a consistent view.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`](crate::types::error::SovdError::NotFound)
    /// if the named cycle is not currently active.
    async fn end_cycle(&self, name: CycleName) -> Result<()>;

    /// Subscribe to cycle events.
    ///
    /// The returned [`watch::Receiver`] is a broadcast channel keyed on
    /// the latest event. Subscribers see the initial state
    /// ([`OperationCycleEvent::Idle`] or whatever is current) immediately
    /// on `borrow()`, then each `changed().await` wakes on the next
    /// transition. Use [`watch::Receiver::borrow_and_update`] to observe
    /// and acknowledge.
    ///
    /// `watch` is chosen over `broadcast` because operation-cycle events
    /// are state transitions, not independent messages — late subscribers
    /// should see the current state, not replay every historical edge.
    async fn subscribe_events(&self) -> watch::Receiver<OperationCycleEvent>;
}
