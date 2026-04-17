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

//! Spec-derived fault entry shapes.
//!
//! Provenance:
//! - `faults/types.yaml#Fault`
//! - `faults/types.yaml#ListOfFaults`
//! - `faults/responses.yaml#FaultDetails` (inline schema)
//! - `faults/parameters.yaml` (`FilterByStatus`, `FilterBySeverity`, `Scope`)
//!
//! The SOVD spec uses the term **fault** for what UDS / classic ECUs call a
//! **DTC**. The on-disk fault `code` is a free-form string, not a 24-bit
//! integer (it can be `"0012E3"`, `"P102"`, `"modelMissing"`, etc.).
//!
//! The `status` field on a fault is an open `object` of OEM-specific
//! key/value pairs. We carry it as `serde_json::Value`. The classic-ECU
//! example in the spec uses keys mirroring the UDS DTC status byte
//! (`testFailed`, `pendingDTC`, `confirmedDTC`, …) but this is convention,
//! not contract.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::spec::error::DataError;

/// A single SOVD fault entry.
///
/// Provenance: `faults/types.yaml#Fault`.
///
/// `code` and `fault_name` are required by the spec. Every other field is
/// optional and may be omitted by the entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Fault {
    /// Native fault code, e.g. `"0012E3"`, `"P102"`, `"modelMissing"`.
    pub code: String,

    /// Scope of the fault (e.g. user-defined fault memory). Capability-defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Display representation of `code`, e.g. `"P102"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_code: Option<String>,

    /// Human-readable fault name.
    pub fault_name: String,

    /// Identifier for translating `fault_name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_translation_id: Option<String>,

    /// Severity. Spec convention: `1=FATAL, 2=ERROR, 3=WARN, 4=INFO`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<i32>,

    /// OEM-specific status key/value object. For classic ECUs typically
    /// includes the UDS DTC status-byte bits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<serde_json::Value>,

    /// OEM-specific symptom / failure-mode description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symptom: Option<String>,

    /// Identifier for translating `symptom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symptom_translation_id: Option<String>,

    /// Tags attached to this fault entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Response body for `GET /{entity-collection}/{entity-id}/faults`.
///
/// Provenance: `faults/types.yaml#ListOfFaults`.
///
/// `schema` is only present if the request was made with
/// `?include-schema=true`; we carry it as an opaque JSON value to avoid
/// modelling the full `OpenAPI` 3.1 meta-schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListOfFaults {
    /// All faults that match the request filter.
    pub items: Vec<Fault>,

    /// Total number of faults that matched the filter before paging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Cursor for the next page, expressed as the next 1-based page number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_page: Option<u32>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Soft-fail marker per ADR-0018 rule 4. Absent on the nominal
    /// path so the spec-pure shape is preserved; set to
    /// `Some(ResponseExtras { stale: true, .. })` when the backend
    /// returned a last-known snapshot because a fresh query failed
    /// or a retry budget was exhausted.
    ///
    /// Extra (per ADR-0006): not part of ISO 17978-3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<crate::extras::response::ResponseExtras>,
}

/// Response body for `GET /{entity-collection}/{entity-id}/faults/{fault-code}`.
///
/// Provenance: `faults/responses.yaml#FaultDetails` (inline schema — the
/// spec does not give it a name in `components/schemas`, so we coin one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FaultDetails {
    /// The fault entry itself.
    pub item: Fault,

    /// OEM-specific environment / freeze-frame data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_data: Option<serde_json::Value>,

    /// Errors related to `environment_data`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<DataError>>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Soft-fail marker per ADR-0018 rule 4. Absent on the nominal
    /// path; set with `stale: true` when a degraded response is
    /// served. Extra (per ADR-0006): not part of ISO 17978-3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<crate::extras::response::ResponseExtras>,
}

/// Filter applied to `GET .../faults`.
///
/// Provenance: combined view of `faults/parameters.yaml` query parameters.
///
/// The spec allows three independent filters:
/// - `status[key]` (repeated query parameter; entries are OR-combined)
/// - `severity` (integer; matches faults strictly below the given level)
/// - `scope` (string; one of the entity-supported scopes)
///
/// We model it as a struct rather than a flat tuple so that future additions
/// (e.g. a future `from_timestamp` parameter) do not break call sites.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FaultFilter {
    /// Status-key matches. Each entry is `(key, value)`. Entries are
    /// OR-combined: a fault matches if **any** entry matches its status
    /// object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status_keys: Vec<(String, String)>,

    /// Severity threshold. A fault matches if `fault.severity < severity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<i32>,

    /// Scope (capability-defined string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl FaultFilter {
    /// Empty filter — matches every fault.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fault() -> Fault {
        Fault {
            code: "0012E3".into(),
            scope: Some("Default".into()),
            display_code: Some("P102".into()),
            fault_name: "No signal from sensor".into(),
            fault_translation_id: Some("CAMERA_0012E3_tid".into()),
            severity: Some(1),
            status: Some(serde_json::json!({
                "testFailed": "1",
                "confirmedDTC": "1",
                "aggregatedStatus": "active",
            })),
            symptom: None,
            symptom_translation_id: None,
            tags: None,
        }
    }

    #[test]
    fn fault_round_trip() {
        let f = sample_fault();
        let json = serde_json::to_string(&f).expect("serialize");
        let back: Fault = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(f, back);
    }

    #[test]
    fn fault_minimal_round_trip() {
        // Only the two required fields per spec.
        let json = r#"{"code":"X1","fault_name":"oops"}"#;
        let parsed: Fault = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.code, "X1");
        assert_eq!(parsed.fault_name, "oops");
        assert!(parsed.severity.is_none());
        assert!(parsed.status.is_none());
    }

    #[test]
    fn list_of_faults_round_trip() {
        let list = ListOfFaults {
            items: vec![sample_fault()],
            total: Some(1),
            next_page: None,
            schema: None,
            extras: None,
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: ListOfFaults = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(list, back);
    }

    #[test]
    fn fault_details_round_trip() {
        let details = FaultDetails {
            item: sample_fault(),
            environment_data: Some(serde_json::json!({
                "id": "env_data",
                "data": {
                    "battery_voltage": 12.8,
                    "occurence_counter": 12i32,
                },
            })),
            errors: None,
            schema: None,
            extras: None,
        };
        let json = serde_json::to_string(&details).expect("serialize");
        let back: FaultDetails = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(details, back);
    }

    #[test]
    fn fault_filter_default_is_all() {
        let f = FaultFilter::all();
        assert!(f.status_keys.is_empty());
        assert!(f.severity.is_none());
        assert!(f.scope.is_none());
    }

    // D2-red: ADR-0018 rules 1 and 4 require every soft-fail response
    // to carry a `stale: true` marker the tester can key off. Rather
    // than invent a third wire shape, we attach an optional
    // `ResponseExtras` blob to `ListOfFaults` and `FaultDetails` so
    // the spec-nominal shape is unchanged but the degraded path can
    // still advertise itself.
    #[test]
    fn list_of_faults_accepts_extras_stale_flag() {
        let list = ListOfFaults {
            items: Vec::new(),
            total: Some(75),
            next_page: Some(2),
            schema: None,
            extras: Some(crate::extras::response::ResponseExtras {
                stale: true,
                age_ms: Some(7_500),
                host_unreachable: None,
            }),
        };
        let json = serde_json::to_string(&list).expect("serialize");
        assert!(json.contains("\"stale\":true"));
        assert!(json.contains("\"age_ms\":7500"));
        assert!(json.contains("\"total\":75"));
        assert!(json.contains("\"next_page\":2"));
        let back: ListOfFaults = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.extras.as_ref().map(|e| e.stale), Some(true));
        assert_eq!(back.total, Some(75));
        assert_eq!(back.next_page, Some(2));
    }

    #[test]
    fn fault_details_accepts_extras_stale_flag() {
        let details = FaultDetails {
            item: sample_fault(),
            environment_data: None,
            errors: None,
            schema: None,
            extras: Some(crate::extras::response::ResponseExtras {
                stale: true,
                age_ms: Some(42),
                host_unreachable: None,
            }),
        };
        let json = serde_json::to_string(&details).expect("serialize");
        assert!(json.contains("\"stale\":true"));
        let back: FaultDetails = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            back.extras.as_ref().and_then(|e| e.age_ms),
            Some(42),
            "age_ms lost round trip"
        );
    }

    #[test]
    fn list_of_faults_without_extras_does_not_emit_field() {
        // Nominal shape stays spec-pure: extras is #[skip_serializing_if]
        // so a non-degraded response matches the ISO 17978-3 schema
        // exactly.
        let list = ListOfFaults {
            items: Vec::new(),
            total: None,
            next_page: None,
            schema: None,
            extras: None,
        };
        let json = serde_json::to_string(&list).expect("serialize");
        assert!(
            !json.contains("extras"),
            "extras field must not leak on nominal path: {json}"
        );
    }

    #[test]
    fn fault_filter_round_trip() {
        let filter = FaultFilter {
            status_keys: vec![
                ("aggregatedStatus".into(), "active".into()),
                ("confirmedDTC".into(), "1".into()),
            ],
            severity: Some(3),
            scope: Some("Default".into()),
        };
        let json = serde_json::to_string(&filter).expect("serialize");
        let back: FaultFilter = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(filter, back);
    }
}
