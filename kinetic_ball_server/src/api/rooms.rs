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
#[derive(Deserialize)]
pub struct DeleteRoomQuery {
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

/// GET /api/rooms - List all open rooms
/// !TODO add filter my_rooms
async fn list_rooms(State(state): State<AppState>) -> Json<Vec<RoomInfo>> {
    let rooms = state.list_rooms().await;
    Json(rooms)
}

/// GET /api/rooms/:id - Get a specific room
async fn get_room(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RoomInfo>, (StatusCode, String)> {
    match state.get_room(&id).await {
        Some(room) => Ok(Json(room)),
        None => Err((StatusCode::NOT_FOUND, format!("Room '{}' not found", id))),
    }
}

/// POST /api/rooms - Create a new room
async fn create_room(
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

/// DELETE /api/rooms/:id?token=X - Delete a room
async fn delete_room(
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
