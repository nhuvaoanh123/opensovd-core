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

//! Component (ECU / device view) identifiers and descriptive metadata.
//!
//! A `ComponentId` maps one-to-one to the SOVD entity path segment
//! `components/{ecu}`. Each component is served by exactly one backend
//! (see [`crate::traits::backend::SovdBackend`]).

use serde::{Deserialize, Serialize};

/// Stable identifier for one SOVD component (ECU or device view).
///
/// Must be URL-safe; the string is embedded directly into SOVD REST paths
/// (`/sovd/v1/components/{id}/...`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentId(pub String);

impl ComponentId {
    /// Build a component id from anything that converts into a `String`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the inner id as a `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Hardware revision string (free-form; vendor-defined encoding).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwRevision(pub String);

/// Software version string (free-form; typically semver or vendor-defined).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwVersion(pub String);

/// Static-ish metadata describing a component.
///
/// Surfaced through SOVD `GET /sovd/v1/components/{id}` and aggregated by
/// `sovd-gateway` when answering `GET /sovd/v1/components`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentInfo {
    /// Stable identifier.
    pub id: ComponentId,
    /// Human-readable name.
    pub name: String,
    /// Hardware revision reported by the ECU, if available.
    pub hw_revision: Option<HwRevision>,
    /// Software version reported by the ECU, if available.
    pub sw_version: Option<SwVersion>,
    /// Free-form vendor string (OEM, supplier, variant).
    pub vendor: Option<String>,
}
