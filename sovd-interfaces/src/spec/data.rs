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

//! Spec-derived data-resource types.
//!
//! Provenance:
//! - `commons/types.yaml#Value`
//! - `commons/types.yaml#ListOfValues`
//! - `commons/types.yaml#ValueMetadata`
//! - `commons/types.yaml#ReadValue`
//! - `commons/types.yaml#DataCategory`
//! - `commons/types.yaml#Severity`
//! - `data/types.yaml#DataCategoryInformation`
//! - `data/types.yaml#ValueGroup`
//! - `data/types.yaml#DataListEntry`
//!
//! These types back the SOVD `data` and `data-lists` endpoints — the SOVD
//! equivalent of UDS `0x22 ReadDataByIdentifier` for both classic and
//! native ECUs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::spec::error::DataError;

/// SOVD severity levels for log-style values.
///
/// Provenance: `commons/types.yaml#Severity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fatal — system cannot continue.
    Fatal,
    /// Error — operation failed.
    Error,
    /// Warning — recoverable issue.
    Warn,
    /// Informational.
    Info,
    /// Debug-only.
    Debug,
}

/// Metadata describing a data value.
///
/// Provenance: `commons/types.yaml#ValueMetadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ValueMetadata {
    /// Stable identifier for the value (e.g. `"DriverWindow"`).
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Category, e.g. `currentData`, `identData`, `sysInfo`, or
    /// `x-<oem>-…`. The spec restricts the string to URL-safe characters
    /// matching `^[A-Za-z0-9_-]+$`; we do not enforce that at type level.
    pub category: String,

    /// Group identifiers the value belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,

    /// Tags attached to this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// One read result from the SOVD `data` endpoint.
///
/// Provenance: `commons/types.yaml#Value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Value {
    /// Stable identifier for the value.
    pub id: String,

    /// Retrieved value payload (open `object` per spec).
    pub data: serde_json::Value,

    /// Metadata describing the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ValueMetadata>,

    /// Per-value error (only set if reading this single value failed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DataError>,
}

/// Response body for `GET .../data-lists/{data-list-id}`.
///
/// Provenance: `commons/types.yaml#ListOfValues`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListOfValues {
    /// All values returned in this read batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Value>>,
}

/// Single-value read result returned from `GET .../data/{data-id}`.
///
/// Provenance: `commons/types.yaml#ReadValue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReadValue {
    /// Stable identifier for the value.
    pub id: String,

    /// Decoded value payload (`AnyValue` per spec — open).
    pub data: serde_json::Value,

    /// Errors encountered while reading parts of `data`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<DataError>>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Description of one supported data category.
///
/// Provenance: `data/types.yaml#DataCategoryInformation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DataCategoryInformation {
    /// Category name (free string; URL-safe per spec pattern).
    pub item: String,

    /// Identifier for translating the category name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_translation_id: Option<String>,
}

/// Description of one value group.
///
/// Provenance: `data/types.yaml#ValueGroup`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ValueGroup {
    /// Stable identifier for the group, unique across all categories.
    pub id: String,

    /// Category the group is defined for.
    pub category: String,

    /// Identifier for translating `category`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_translation_id: Option<String>,

    /// Display name of the group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// Identifier for translating `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_translation_id: Option<String>,

    /// Tags attached to the group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Description of one specific data list provided by an entity.
///
/// Provenance: `data/types.yaml#DataListEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DataListEntry {
    /// Stable identifier for the data list.
    pub id: String,

    /// Tags attached to this data list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    /// Members of the data list (each entry is one value's metadata).
    pub items: Vec<ValueMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_round_trip() {
        for s in [
            Severity::Fatal,
            Severity::Error,
            Severity::Warn,
            Severity::Info,
            Severity::Debug,
        ] {
            let json = serde_json::to_string(&s).expect("serialize");
            let back: Severity = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(s, back);
        }
    }

    #[test]
    fn value_metadata_round_trip() {
        let m = ValueMetadata {
            id: "DriverWindow".into(),
            name: "Position of driver window".into(),
            translation_id: None,
            category: "currentData".into(),
            groups: Some(vec!["front".into()]),
            tags: None,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: ValueMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn value_round_trip() {
        let v = Value {
            id: "battery_voltage".into(),
            data: serde_json::json!({"value": 12.8f64, "unit": "V"}),
            metadata: None,
            error: None,
        };
        let json = serde_json::to_string(&v).expect("serialize");
        let back: Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v, back);
    }

    #[test]
    fn read_value_round_trip() {
        let r = ReadValue {
            id: "vin".into(),
            data: serde_json::json!("WDD2031411F123456"),
            errors: None,
            schema: None,
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: ReadValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn data_list_entry_round_trip() {
        let entry = DataListEntry {
            id: "front-windows".into(),
            tags: None,
            items: vec![ValueMetadata {
                id: "DriverWindow".into(),
                name: "Driver window".into(),
                translation_id: None,
                category: "currentData".into(),
                groups: None,
                tags: None,
            }],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: DataListEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }
}
