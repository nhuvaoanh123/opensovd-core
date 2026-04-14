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

//! Discovery endpoints — `/sovd/v1/components` and per-component entity
//! capabilities.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use sovd_interfaces::{
    ComponentId,
    spec::component::{DiscoveredEntities, EntityCapabilities},
};

use crate::{InMemoryServer, routes::error::ApiError};

/// `GET /sovd/v1/components` — list every demo component.
///
/// # Errors
///
/// Maps any [`SovdError`](sovd_interfaces::SovdError) from the underlying
/// [`InMemoryServer`] onto an HTTP error via [`ApiError`].
#[utoipa::path(
    get,
    path = "/sovd/v1/components",
    operation_id = "listComponents",
    tag = "discovery",
    responses(
        (status = 200, description = "List of registered SOVD components", body = DiscoveredEntities),
    ),
)]
pub async fn list_components(
    State(server): State<Arc<InMemoryServer>>,
) -> Result<Json<DiscoveredEntities>, ApiError> {
    Ok(Json(server.list_entities().await?))
}

/// `GET /sovd/v1/components/{component_id}` — entity capabilities.
///
/// # Errors
///
/// Returns 404 if the component is not registered; any other
/// [`SovdError`](sovd_interfaces::SovdError) is mapped via [`ApiError`].
#[utoipa::path(
    get,
    path = "/sovd/v1/components/{component_id}",
    operation_id = "getComponent",
    tag = "discovery",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
    ),
    responses(
        (status = 200, description = "Entity capability summary", body = EntityCapabilities),
        (status = 404, description = "Component not found"),
    ),
)]
pub async fn get_component(
    State(server): State<Arc<InMemoryServer>>,
    Path(component_id): Path<String>,
) -> Result<Json<EntityCapabilities>, ApiError> {
    let component = ComponentId::new(component_id);
    Ok(Json(server.dispatch_entity_capabilities(&component).await?))
}
