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

//! Soft-fail response markers — per ADR-0018 rule 4 and rule 5.
//!
//! Extra (per ADR-0006): the ISO 17978-3 spec has no concept of a
//! "stale" or "partially-degraded" response. ADR-0018 ratifies the
//! upstream CDA "never hard fail" principle: backends that cannot
//! produce a full-strength response fall back to a last-known
//! snapshot, a retry budget exhaustion, or a partial fan-out — and
//! the tester session stays alive. That behaviour needs a wire
//! marker so the tester can distinguish "current" from "degraded"
//! data. This module carries that marker.
//!
//! The marker is attached as `extras: Option<ResponseExtras>` on the
//! spec fault types. On the nominal path it serialises as absent (no
//! field), preserving the spec-pure shape. On the degraded path it
//! carries a short set of machine-readable flags the HIL runner can
//! key off.
//!
//! See `spec::fault::ListOfFaults` and `spec::fault::FaultDetails`
//! for the current consumers.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::component::ComponentId;

/// Component-id wire form used inside `ResponseExtras.host_unreachable`.
/// `ComponentId` itself is serde-transparent but does not derive
/// `utoipa::ToSchema`, so we carry the wire as a plain string list
/// here and convert at the call sites.
type ComponentIdWire = String;

/// Soft-fail metadata attached to a SOVD response body per ADR-0018.
///
/// Extra (per ADR-0006): not part of ISO 17978-3. Carried as an
/// optional, flatten-friendly struct under the `extras` field of
/// select response types.
///
/// # Field conventions
///
/// - `stale` is `true` when the backend returned something other
///   than a fresh, authoritative result (e.g. the last-known cache
///   snapshot, a degraded retry result, or an aggregated fan-out
///   with one host missing).
/// - `age_ms` is set when the response came from a cache; it is the
///   age of the snapshot in milliseconds.
/// - `host_unreachable` is a sorted list of component ids whose
///   remote host could not be reached during a `sovd-gateway`
///   fan-out. An empty list or `None` means "no known partial
///   failures."
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResponseExtras {
    /// `true` when this response is a degraded / last-known fallback
    /// rather than a full-strength answer.
    #[serde(default)]
    pub stale: bool,

    /// Age of the cached snapshot in milliseconds, when `stale` was
    /// set because a cache was served instead of fresh data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,

    /// Component ids whose backing remote host was unreachable during
    /// this request. Used by `sovd-gateway::list_components` fan-out
    /// per ADR-0018 rule 5.
    ///
    /// Wire type is `Vec<String>` (not `Vec<ComponentId>`) because
    /// `ComponentId` does not derive `utoipa::ToSchema`; call sites
    /// that need the typed form convert via
    /// [`ResponseExtras::host_unreachable_typed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_unreachable: Option<Vec<ComponentIdWire>>,
}

impl ResponseExtras {
    /// Build a stale-cache marker with the given age.
    #[must_use]
    pub const fn stale_cache(age_ms: u64) -> Self {
        Self {
            stale: true,
            age_ms: Some(age_ms),
            host_unreachable: None,
        }
    }

    /// Build a generic degraded marker with no cache age.
    #[must_use]
    pub const fn degraded() -> Self {
        Self {
            stale: true,
            age_ms: None,
            host_unreachable: None,
        }
    }

    /// Convert the wire-form `host_unreachable` list into typed
    /// [`ComponentId`]s for in-process consumers.
    #[must_use]
    pub fn host_unreachable_typed(&self) -> Vec<ComponentId> {
        self.host_unreachable
            .as_ref()
            .map(|v| v.iter().cloned().map(ComponentId::new).collect())
            .unwrap_or_default()
    }

    /// Build a host-unreachable marker from typed component ids.
    #[must_use]
    pub fn host_unreachable(ids: Vec<ComponentId>) -> Self {
        Self {
            stale: true,
            age_ms: None,
            host_unreachable: Some(ids.into_iter().map(|c| c.as_str().to_owned()).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_nominal() {
        let e = ResponseExtras::default();
        assert!(!e.stale);
        assert!(e.age_ms.is_none());
        assert!(e.host_unreachable.is_none());
    }

    #[test]
    fn stale_cache_helper_sets_flags() {
        let e = ResponseExtras::stale_cache(120);
        assert!(e.stale);
        assert_eq!(e.age_ms, Some(120));
    }

    #[test]
    fn round_trip_json() {
        let e = ResponseExtras {
            stale: true,
            age_ms: Some(99),
            host_unreachable: Some(vec!["cvc".to_owned()]),
        };
        let json = serde_json::to_string(&e).expect("serialize");
        let back: ResponseExtras = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }

    #[test]
    fn host_unreachable_typed_round_trip() {
        let e = ResponseExtras::host_unreachable(vec![
            ComponentId::new("cvc"),
            ComponentId::new("sc"),
        ]);
        let typed = e.host_unreachable_typed();
        assert_eq!(
            typed,
            vec![ComponentId::new("cvc"), ComponentId::new("sc")]
        );
        assert!(e.stale);
    }
}
