use bevy::prelude::*;

// ============================================================================
// ESTADOS DE LA APLICACIÓN
// ============================================================================

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    Menu,
    Settings,
    #[default]
    LocalPlayersSetup,
    GamepadConfig,
    RoomSelection,
    CreateRoom,
    HostingRoom,
    Connecting,
    InGame,
}

// ============================================================================
// ROOM INFO (from proxy API)
// ============================================================================

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus {
    Open,
    Full,
    Closed,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub name: String,
    pub max_players: u8,
    pub current_players: u8,
    pub map_name: Option<String>,
    pub status: RoomStatus,
    #[serde(default)]
    pub min_version: Option<String>,
}
