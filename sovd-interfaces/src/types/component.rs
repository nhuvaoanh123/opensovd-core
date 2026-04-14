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

// Component metadata DTOs (former `ComponentInfo`, `HwRevision`, `SwVersion`)
// were removed in the spec port (Deliverable 3, 2026-04-14). The wire
// equivalent is now [`crate::spec::component::EntityCapabilities`], ported
// directly from `discovery/responses.yaml#discoveredEntityCapabilities`.
