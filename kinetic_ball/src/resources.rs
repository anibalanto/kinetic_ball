use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::Material2d;
use std::f32::consts::FRAC_PI_2;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::assets::SPLIT_SCREEN_SHADER_HANDLE;
use crate::keybindings::AppConfig;
use crate::shared::match_slots::MatchSlots;
use crate::shared::protocol::{ControlMessage, PlayerInput, ServerMessage};
use crate::states::RoomInfo;

// ============================================================================
// GAME STATE RESOURCES
// ============================================================================

#[derive(Resource, Default)]
pub struct GameTick(pub u32);

/// Estado del panel de administración en el juego
#[derive(Resource, Default)]
pub struct AdminPanelState {
    pub is_open: bool,
    /// Whether the local player is an admin (can move players)
    pub is_admin: bool,
}

/// Client-side copy of match slots, synchronized from server
#[derive(Resource, Default)]
pub struct ClientMatchSlots(pub MatchSlots);

/// Solicitud para salir de la sala (se procesa en cleanup)
#[derive(Resource, Default)]
pub struct LeaveRoomRequest {
    pub pending: bool,
    pub player_ids: Vec<u32>,
}

// Resource para trackear el input anterior (legacy, mantenido por compatibilidad)
#[derive(Resource, Default)]
pub struct PreviousInput(pub PlayerInput);

// ============================================================================
// NETWORK RESOURCES
// ============================================================================

/// Canal de comunicación con el thread de red
/// El sender ahora envía (player_id, PlayerInput) para soportar múltiples jugadores locales
#[derive(Resource, Default)]
pub struct NetworkChannels {
    pub receiver: Option<Arc<Mutex<mpsc::Receiver<ServerMessage>>>>,
    pub sender: Option<mpsc::Sender<(u32, PlayerInput)>>,
    /// Canal para enviar mensajes de control (Leave, etc.)
    pub control_sender: Option<mpsc::Sender<ControlMessage>>,
}

#[derive(Resource)]
pub struct MyPlayerId(pub Option<u32>);

// ============================================================================
// MAP RESOURCES
// ============================================================================

#[derive(Resource, Default)]
pub struct LoadedMap(pub Option<crate::shared::map::Map>);

// ============================================================================
// PLAYER COLOR RESOURCES
// ============================================================================

/// Colores únicos para cada jugador en el minimapa y nombres
#[derive(Resource, Default)]
pub struct PlayerColors {
    pub colors: std::collections::HashMap<u32, Color>, // server_player_id -> Color
    pub next_hue_offset: f32,
}

// ============================================================================
// ROOM RESOURCES
// ============================================================================

#[derive(Resource)]
pub struct RoomList {
    pub rooms: Vec<RoomInfo>,
    pub loading: bool,
    pub error: Option<String>,
    // Filtros
    pub filter_name: String,
    pub filter_my_hosts_only: bool,
    pub filter_show_full: bool,
    pub filter_show_available: bool,
    // Conexión directa por UUID
    pub direct_connect_id: String,
}

impl Default for RoomList {
    fn default() -> Self {
        Self {
            rooms: Vec::new(),
            loading: false,
            error: None,
            filter_name: String::new(),
            filter_my_hosts_only: false,
            filter_show_full: true,
            filter_show_available: true,
            direct_connect_id: String::new(),
        }
    }
}

#[derive(Resource, Default)]
pub struct RoomFetchChannel {
    pub receiver: Option<Arc<Mutex<mpsc::Receiver<Result<Vec<RoomInfo>, String>>>>>,
}

#[derive(Resource, Default)]
pub struct SelectedRoom {
    pub room_id: Option<String>,
}

#[derive(Resource)]
pub struct CreateRoomConfig {
    pub room_name: String,
    pub max_players: u8,
    pub map_path: String,
    pub scale: f32,
    pub created_room_ids: Vec<String>,
}

impl Default for CreateRoomConfig {
    fn default() -> Self {
        Self {
            room_name: String::from("mi_sala"),
            max_players: 4,
            map_path: String::new(), // Vacío = usar mapa embebido por defecto
            scale: 1.0,
            created_room_ids: Vec::new(),
        }
    }
}

// ============================================================================
// CONNECTION CONFIG
// ============================================================================

#[derive(Resource)]
pub struct ConnectionConfig {
    pub server_host: String, // Host sin protocolo: localhost:3536 o api.example.com
    pub room: String,
    pub player_name: String,
}

impl ConnectionConfig {
    pub fn from_args(args: &crate::Args, app_config: &AppConfig) -> Self {
        Self {
            server_host: args
                .server
                .clone()
                .unwrap_or_else(|| app_config.server.clone()),
            room: args.room.clone(),
            player_name: args.name.clone(),
        }
    }

    /// Determina si debe usar conexión segura (HTTPS/WSS)
    pub fn is_secure(&self) -> bool {
        let host = &self.server_host;
        // Usar HTTP/WS solo para desarrollo local
        // Todo lo demás usa HTTPS/WSS
        !host.starts_with("localhost") && !host.starts_with("127.0.0.1")
    }

    /// URL HTTP/HTTPS para llamadas REST API
    pub fn http_url(&self) -> String {
        let protocol = if self.is_secure() { "https" } else { "http" };
        format!("{}://{}", protocol, self.server_host)
    }

    /// URL WebSocket WS/WSS para conexiones WS
    pub fn ws_url(&self) -> String {
        let protocol = if self.is_secure() { "wss" } else { "ws" };
        format!("{}://{}", protocol, self.server_host)
    }
}

// ============================================================================
// DYNAMIC SPLIT-SCREEN RESOURCES
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitMode {
    #[default]
    Unified, // Una sola cámara siguiendo a ambos jugadores
    Transitioning, // Animando entre modos
    Split,         // Dos cámaras independientes
}

#[derive(Resource)]
pub struct DynamicSplitState {
    pub mode: SplitMode,
    pub split_factor: f32,    // 0.0 = unified, 1.0 = full split
    pub split_angle: f32,     // Ángulo de la línea divisoria en radianes
    pub merge_threshold: f32, // Distancia para fusionar (con histéresis)
    pub split_threshold: f32, // Distancia para separar
    /// Ratio del viewport visible (basado en zoom) usado para calcular umbrales
    pub viewport_visible_ratio: f32,
}

impl Default for DynamicSplitState {
    fn default() -> Self {
        Self {
            mode: SplitMode::Unified,
            split_factor: 0.0,
            split_angle: FRAC_PI_2, // Vertical por defecto
            merge_threshold: 600.0, // Distancia a la que se fusionan
            split_threshold: 800.0, // Distancia a la que se separan
            viewport_visible_ratio: 1.0,
        }
    }
}

/// Handles para las texturas de render target de cada cámara
#[derive(Resource, Default)]
pub struct SplitScreenTextures {
    pub camera1_texture: Option<Handle<Image>>,
    pub camera2_texture: Option<Handle<Image>>,
}

/// Material para el compositor de split-screen
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SplitScreenMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub camera1_texture: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    pub camera2_texture: Handle<Image>,
    /// x: angle, y: factor, z: center_x, w: center_y
    #[uniform(4)]
    pub split_params: Vec4,
}

impl Material2d for SplitScreenMaterial {
    fn fragment_shader() -> ShaderRef {
        SPLIT_SCREEN_SHADER_HANDLE.into()
    }
}
