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

//! Spec-derived discovery / component types.
//!
//! Provenance:
//! - `commons/types.yaml#EntityReference`
//! - `commons/parameters.yaml#EntityCollection`
//! - `discovery/responses.yaml#discoveredEntities` (inline schema)
//! - `discovery/responses.yaml#discoveredEntitiesWithSchema` (inline schema)
//! - `discovery/responses.yaml#discoveredEntityCapabilities` (inline schema —
//!   no spec-given name; we coin [`EntityCapabilities`])
//!
//! In the SOVD spec, `components` is one of four entity collections —
//! `areas`, `components`, `apps`, `functions`. The Phase-3/4 MVP only deals
//! with `components`, but the types are collection-agnostic.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which kind of entity collection an entity lives in.
///
/// Provenance: `commons/parameters.yaml#EntityCollection` (path parameter,
/// schema enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntityCollection {
    /// `/areas`
    Areas,
    /// `/components` — physical / logical ECUs.
    Components,
    /// `/apps`
    Apps,
    /// `/functions`
    Functions,
}

impl EntityCollection {
    /// URL-segment string for this collection.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Areas => "areas",
            Self::Components => "components",
            Self::Apps => "apps",
            Self::Functions => "functions",
        }
    }
}

/// Reference to one entity in a discovery list.
///
/// Provenance: `commons/types.yaml#EntityReference`.
///
/// `id`, `name`, and `href` are required by the spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EntityReference {
    /// Stable identifier for the entity.
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Absolute URI of the entity (including `{base_uri}`).
    pub href: String,

    /// Tags attached to this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Response body for `GET /{entity-collection}` (no inline schema).
///
/// Provenance: `discovery/responses.yaml#discoveredEntities` (inline schema).
///
/// Used for the simpler list endpoints that do not also expose
/// `?include-schema=true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DiscoveredEntities {
    /// All entities in the collection.
    pub items: Vec<EntityReference>,

    /// Soft-fail marker per ADR-0018 rule 5. Set by the
    /// `sovd-gateway` fan-out aggregator to report that one or more
    /// remote hosts were unreachable but the remaining hosts still
    /// answered. Absent on the nominal path.
    ///
    /// Extra (per ADR-0006): not part of ISO 17978-3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<crate::extras::response::ResponseExtras>,
}

/// Response body for `GET /{entity-collection}` with optional schema embed.
///
/// Provenance: `discovery/responses.yaml#discoveredEntitiesWithSchema`
/// (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DiscoveredEntitiesWithSchema {
    /// All entities in the collection.
    pub items: Vec<EntityReference>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Response body for `GET /{entity-collection}/{entity-id}`.
///
/// Provenance: `discovery/responses.yaml#discoveredEntityCapabilities`
/// (inline schema — the spec does not name it; we coin
/// `EntityCapabilities`).
///
/// Most of the fields are URI references to subordinate resource
/// collections (`faults`, `data`, `operations`, …) and are only set if
/// the entity supports that resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EntityCapabilities {
    /// Entity-Id, e.g. `"AdvancedLaneKeeping"`.
    pub id: String,

    /// Human-readable name of the entity.
    pub name: String,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Variant identification (open structure per spec — `AnyValue`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<serde_json::Value>,

    /// URI to the configurations sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configurations: Option<String>,

    /// URI to the bulk-data sub-collection.
    #[serde(default, rename = "bulk-data", skip_serializing_if = "Option::is_none")]
    pub bulk_data: Option<String>,

    /// URI to the data sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,

    /// URI to the data-lists sub-collection.
    #[serde(
        default,
        rename = "data-lists",
        skip_serializing_if = "Option::is_none"
    )]
    pub data_lists: Option<String>,

    /// URI to the faults sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faults: Option<String>,

    /// URI to the operations sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<String>,

    /// URI to the updates sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates: Option<String>,

    /// URI to the modes sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<String>,

    /// URI to the subareas sub-collection (areas only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subareas: Option<String>,

    /// URI to the subcomponents sub-collection (components only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcomponents: Option<String>,

    /// URI to the locks sub-collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locks: Option<String>,

    /// URI to the depends-on reference collection.
    #[serde(
        default,
        rename = "depends-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub depends_on: Option<String>,

    /// URI to the hosts reference collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosts: Option<String>,

    /// URI to the parent component (apps only).
    #[serde(
        default,
        rename = "is-located-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_located_on: Option<String>,

    /// URI to the scripts collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<String>,

    /// URI to the logs collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_collection_str() {
        assert_eq!(EntityCollection::Areas.as_str(), "areas");
        assert_eq!(EntityCollection::Components.as_str(), "components");
        assert_eq!(EntityCollection::Apps.as_str(), "apps");
        assert_eq!(EntityCollection::Functions.as_str(), "functions");
    }

    #[test]
    fn entity_collection_round_trip() {
        for kind in [
            EntityCollection::Areas,
            EntityCollection::Components,
            EntityCollection::Apps,
            EntityCollection::Functions,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            let back: EntityCollection = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn entity_reference_round_trip() {
        let r = EntityReference {
            id: "alk".into(),
            name: "Advanced Lane Keeping".into(),
            translation_id: Some("alk_tid".into()),
            href: "https://sovd.server/v1/components/alk".into(),
            tags: Some(vec!["adas".into(), "safety".into()]),
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: EntityReference = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn entity_reference_minimal() {
        let json = r#"{"id":"x","name":"X","href":"/v1/components/x"}"#;
        let parsed: EntityReference = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.id, "x");
        assert!(parsed.tags.is_none());
        assert!(parsed.translation_id.is_none());
    }

    #[test]
    fn discovered_entities_round_trip() {
        let d = DiscoveredEntities {
            items: vec![EntityReference {
                id: "alk".into(),
                name: "ALK".into(),
                translation_id: None,
                href: "/v1/components/alk".into(),
                tags: None,
            }],
            extras: None,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        let back: DiscoveredEntities = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
    }

    #[test]
    fn entity_capabilities_round_trip_full() {
        let cap = EntityCapabilities {
            id: "AdvancedLaneKeeping".into(),
            name: "Advanced Lane Keeping".into(),
            translation_id: None,
            variant: None,
            configurations: Some("https://sovd.server/v1/apps/alk/configurations".into()),
            bulk_data: Some("https://sovd.server/v1/apps/alk/bulk-data".into()),
            data: Some("https://sovd.server/v1/apps/alk/data".into()),
            data_lists: None,
            faults: Some("https://sovd.server/v1/apps/alk/faults".into()),
            operations: Some("https://sovd.server/v1/apps/alk/operations".into()),
            updates: None,
            modes: None,
            subareas: None,
            subcomponents: None,
            locks: None,
            depends_on: None,
            hosts: None,
            is_located_on: None,
            scripts: None,
            logs: Some("https://sovd.server/v1/apps/alk/logs".into()),
        };
        let json = serde_json::to_string(&cap).expect("serialize");
        let back: EntityCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cap, back);
    }

    #[test]
    fn entity_capabilities_kebab_renames() {
        // Verify that fields with `-` in the spec serialise correctly.
        let cap = EntityCapabilities {
            id: "x".into(),
            name: "X".into(),
            translation_id: None,
            variant: None,
            configurations: None,
            bulk_data: Some("/bulk".into()),
            data: None,
            data_lists: Some("/data-lists".into()),
            faults: None,
            operations: None,
            updates: None,
            modes: None,
            subareas: None,
            subcomponents: None,
            locks: None,
            depends_on: Some("/dep".into()),
            hosts: None,
            is_located_on: Some("/loc".into()),
            scripts: None,
            logs: None,
        };
        let json = serde_json::to_string(&cap).expect("serialize");
        assert!(json.contains("\"bulk-data\":\"/bulk\""));
        assert!(json.contains("\"data-lists\":\"/data-lists\""));
        assert!(json.contains("\"depends-on\":\"/dep\""));
        assert!(json.contains("\"is-located-on\":\"/loc\""));
    }
}
