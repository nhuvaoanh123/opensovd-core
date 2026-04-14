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

//! Data identifier (DID) shapes for SOVD `data` reads.
//!
//! Maps to UDS service `0x22 ReadDataByIdentifier` on CDA-backed ECUs and
//! to MDD-backed providers on native `sovd-server` backends. The actual
//! decoding from raw bytes to typed values is done by the backend using
//! ODX/MDD; the interface crate only carries the already-decoded
//! [`DataValue`].

use serde::{Deserialize, Serialize};

/// 16-bit UDS Data Identifier (widened to `u32` for vendor extensions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataIdentifier(pub u32);

impl std::fmt::Display for DataIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

/// Decoded value of a DID, as a JSON value so that arbitrary MDD-defined
/// structured types can flow through the trait boundary without the
/// interface crate knowing the schema.
///
/// Schema introspection (the `?include-schema=true` query param in SOVD)
/// is carried separately on the HTTP layer; the interface trait returns
/// values only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataValue(pub serde_json::Value);
