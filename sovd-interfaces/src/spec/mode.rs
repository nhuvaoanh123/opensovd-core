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

//! Spec-derived target-mode types.
//!
//! Provenance:
//! - `modes/types.yaml#ModeCollectionItem`
//! - `modes/responses.yaml#SupportedModes`
//! - `modes/responses.yaml#ModeDetails`
//! - `modes/responses.yaml#ControlStates`
//!
//! These types back the SOVD `modes` endpoints — the SOVD equivalent of
//! UDS session control, security access, communication control, and DTC
//! setting. Each entity exposes a collection of modes; each mode has a
//! current value that clients can `GET` and (where supported) `PUT`.
//!
//! The four mode identifiers the spec calls out by name are `session`,
//! `security`, `commctrl`, and `dtcsetting`, but the collection is open
//! and OEM-specific modes are allowed.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One entry in a SOVD mode collection.
///
/// Provenance: `modes/types.yaml#ModeCollectionItem`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModeCollectionItem {
    /// Stable identifier for the mode (e.g. `"session"`).
    pub id: String,

    /// Human-readable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Tags attached to this mode. The spec defines the field via the
    /// shared `commons/types.yaml#SupportedTags` alias (array of strings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Response body for `GET .../components/{component}/modes` — the list
/// of all target modes an entity supports.
///
/// Provenance: `modes/responses.yaml#SupportedModes` (inline response
/// schema).
///
/// The `schema` field carries an optional `OpenApiSchema` subtree (via
/// `$ref` in the upstream YAML). It is only populated when the client
/// requests `include-schema=true`, and the spec defines its contents as
/// an arbitrary `OpenAPI` 3.1 schema, so we carry it as free-form
/// `serde_json::Value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SupportedModes {
    /// All modes the entity exposes, each described by its
    /// `ModeCollectionItem`.
    pub items: Vec<ModeCollectionItem>,

    /// Optional embedded `OpenAPI` schema describing the response shape.
    /// Only present when the client requests `include-schema=true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Response body for `GET .../components/{component}/modes/{mode}` —
/// the current value and metadata of one specific mode.
///
/// Provenance: `modes/responses.yaml#ModeDetails` (inline response
/// schema).
///
/// `value` is required; `name` and `translation_id` are optional
/// display metadata. The spec keeps `value` as a plain string here —
/// contrast with `ControlStates` which uses the open `AnyValue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModeDetails {
    /// Human-readable name of the mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Current string value of the mode (e.g. `"DEFAULT"`,
    /// `"EXTENDED"`, `"PROGRAMMING"` for a diagnostic session).
    pub value: String,

    /// Optional embedded `OpenAPI` schema describing the response shape.
    /// Only present when the client requests `include-schema=true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Response body for a single control-state mode query.
///
/// Provenance: `modes/responses.yaml#ControlStates` (inline response
/// schema).
///
/// Both `id` and `value` are required. Unlike `ModeDetails`, `value`
/// here is the spec's open `AnyValue` (`commons/types.yaml#AnyValue`):
/// an `anyOf` of string, number, integer, boolean, array, or object.
/// Because the spec intentionally leaves this polymorphic, we carry it
/// as `serde_json::Value` — **not** as a spec-type escape hatch but
/// because the spec itself defines the field as arbitrary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ControlStates {
    /// Stable identifier of the mode.
    pub id: String,

    /// Current value of the mode, shape per spec `AnyValue`.
    pub value: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_collection_item_round_trip() {
        let m = ModeCollectionItem {
            id: "session".into(),
            name: Some("Diagnostic session".into()),
            translation_id: None,
            tags: Some(vec!["uds".into()]),
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: ModeCollectionItem = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn mode_collection_item_minimal_round_trip() {
        let m = ModeCollectionItem {
            id: "security".into(),
            name: None,
            translation_id: None,
            tags: None,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: ModeCollectionItem = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
        assert_eq!(json, r#"{"id":"security"}"#);
    }

    #[test]
    fn supported_modes_round_trip() {
        let s = SupportedModes {
            items: vec![
                ModeCollectionItem {
                    id: "session".into(),
                    name: Some("Diagnostic session".into()),
                    translation_id: None,
                    tags: None,
                },
                ModeCollectionItem {
                    id: "security".into(),
                    name: Some("Security access".into()),
                    translation_id: None,
                    tags: None,
                },
            ],
            schema: None,
        };
        let json = serde_json::to_string(&s).expect("serialize");
        let back: SupportedModes = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, back);
    }

    #[test]
    fn mode_details_round_trip() {
        let d = ModeDetails {
            name: Some("Diagnostic session".into()),
            translation_id: None,
            value: "DEFAULT".into(),
            schema: None,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        let back: ModeDetails = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
    }

    #[test]
    fn mode_details_minimal_round_trip() {
        let d = ModeDetails {
            name: None,
            translation_id: None,
            value: "EXTENDED".into(),
            schema: None,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        let back: ModeDetails = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
        assert_eq!(json, r#"{"value":"EXTENDED"}"#);
    }

    #[test]
    fn control_states_round_trip_string() {
        let c = ControlStates {
            id: "session".into(),
            value: serde_json::json!("EXTENDED"),
        };
        let json = serde_json::to_string(&c).expect("serialize");
        let back: ControlStates = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(c, back);
    }

    #[test]
    fn control_states_round_trip_object() {
        let c = ControlStates {
            id: "commctrl".into(),
            value: serde_json::json!({"enabled": true, "channels": ["A", "B"]}),
        };
        let json = serde_json::to_string(&c).expect("serialize");
        let back: ControlStates = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(c, back);
    }
}
