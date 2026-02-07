pub mod rooms;

pub use rooms::rooms_router;

use utoipa::OpenApi;

use crate::state::{CreateRoomRequest, CreateRoomResponse, RoomInfo, RoomStatus};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kinetic Ball Server API",
        description = "REST API for managing game rooms in the Kinetic Ball multiplayer server.\n\nAll endpoints require HMAC authentication headers:\n- `X-Client-Version` — client semver version (e.g. \"0.7.1\")\n- `X-Client-Time` — current unix time in minutes (seconds / 60)\n- `X-Client-Token` — HMAC-SHA256 hex digest of \"{version}:{timestamp}\" with the shared secret",
        version = "0.7.1",
        license(name = "MIT-0", url = "https://github.com/anibalanto/kinetic_ball/blob/main/LICENSE"),
    ),
    paths(
        rooms::list_rooms,
        rooms::get_room,
        rooms::create_room,
        rooms::delete_room,
    ),
    components(schemas(RoomInfo, RoomStatus, CreateRoomRequest, CreateRoomResponse)),
    tags(
        (name = "rooms", description = "Game room management operations")
    )
)]
pub struct ApiDoc;
