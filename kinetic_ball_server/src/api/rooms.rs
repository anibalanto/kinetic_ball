use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::state::{AppState, CreateRoomRequest, CreateRoomResponse, RoomInfo};

/// Query params for delete endpoint
#[derive(Deserialize, utoipa::IntoParams)]
pub struct DeleteRoomQuery {
    /// Authentication token for the room
    token: String,
}

/// Create the rooms router
pub fn rooms_router() -> Router<AppState> {
    Router::new()
        .route("/rooms", get(list_rooms))
        .route("/rooms", post(create_room))
        .route("/rooms/:id", get(get_room))
        .route("/rooms/:id", delete(delete_room))
}

/// List all open rooms
#[utoipa::path(
    get,
    path = "/api/rooms",
    params(
        ("X-Client-Version" = String, Header, description = "Client semver version"),
        ("X-Client-Time" = String, Header, description = "Current unix time in minutes"),
        ("X-Client-Token" = String, Header, description = "HMAC-SHA256 hex token"),
    ),
    responses(
        (status = 200, description = "List of active rooms", body = Vec<RoomInfo>),
        (status = 400, description = "Missing HMAC authentication headers"),
        (status = 401, description = "Invalid token or expired timestamp"),
        (status = 426, description = "Client version too old"),
    ),
    tag = "rooms"
)]
pub(crate) async fn list_rooms(State(state): State<AppState>) -> Json<Vec<RoomInfo>> {
    let rooms = state.list_rooms().await;
    Json(rooms)
}

/// Get a specific room by ID
#[utoipa::path(
    get,
    path = "/api/rooms/{id}",
    params(
        ("id" = String, Path, description = "The room ID"),
        ("X-Client-Version" = String, Header, description = "Client semver version"),
        ("X-Client-Time" = String, Header, description = "Current unix time in minutes"),
        ("X-Client-Token" = String, Header, description = "HMAC-SHA256 hex token"),
    ),
    responses(
        (status = 200, description = "Room details", body = RoomInfo),
        (status = 400, description = "Missing HMAC authentication headers"),
        (status = 401, description = "Invalid token or expired timestamp"),
        (status = 404, description = "Room not found"),
        (status = 426, description = "Client version too old"),
    ),
    tag = "rooms"
)]
pub(crate) async fn get_room(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RoomInfo>, (StatusCode, String)> {
    match state.get_room(&id).await {
        Some(room) => Ok(Json(room)),
        None => Err((StatusCode::NOT_FOUND, format!("Room '{}' not found", id))),
    }
}

/// Create a new room
#[utoipa::path(
    post,
    path = "/api/rooms",
    params(
        ("X-Client-Version" = String, Header, description = "Client semver version"),
        ("X-Client-Time" = String, Header, description = "Current unix time in minutes"),
        ("X-Client-Token" = String, Header, description = "HMAC-SHA256 hex token"),
    ),
    request_body = CreateRoomRequest,
    responses(
        (status = 201, description = "Room created successfully", body = CreateRoomResponse),
        (status = 400, description = "Invalid request or missing HMAC headers"),
        (status = 401, description = "Invalid token or expired timestamp"),
        (status = 409, description = "Room already exists"),
        (status = 426, description = "Client version too old"),
    ),
    tag = "rooms"
)]
pub(crate) async fn create_room(
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Validate request
    if request.room_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "room_id cannot be empty".to_string(),
        ));
    }
    if request.name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name cannot be empty".to_string()));
    }
    if request.max_players == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_players must be greater than 0".to_string(),
        ));
    }

    match state.register_room(request).await {
        Ok(token) => {
            tracing::info!("Room created, token generated");
            Ok((StatusCode::CREATED, Json(CreateRoomResponse { token })))
        }
        Err(e) => Err((StatusCode::CONFLICT, e)),
    }
}

/// Delete a room (requires the token returned at creation)
#[utoipa::path(
    delete,
    path = "/api/rooms/{id}",
    params(
        ("id" = String, Path, description = "The room ID to delete"),
        ("X-Client-Version" = String, Header, description = "Client semver version"),
        ("X-Client-Time" = String, Header, description = "Current unix time in minutes"),
        ("X-Client-Token" = String, Header, description = "HMAC-SHA256 hex token"),
        DeleteRoomQuery,
    ),
    responses(
        (status = 204, description = "Room deleted successfully"),
        (status = 400, description = "Missing HMAC authentication headers"),
        (status = 401, description = "Invalid HMAC token or expired timestamp"),
        (status = 403, description = "Invalid or missing room token"),
        (status = 426, description = "Client version too old"),
    ),
    tag = "rooms"
)]
pub(crate) async fn delete_room(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteRoomQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    match state.delete_room(&id, &query.token).await {
        Ok(()) => {
            tracing::info!(room_id = %id, "Room deleted");
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => Err((StatusCode::FORBIDDEN, e)),
    }
}
