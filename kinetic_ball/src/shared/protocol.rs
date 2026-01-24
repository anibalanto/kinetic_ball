use super::map::Map;
use bevy::prelude::{Component, Vec2};
use serde::{Deserialize, Serialize};

// ============================================================================
// NUEVOS MENSAJES PARA WEBRTC (Canales Separados)
// ============================================================================

/// Mensajes críticos que requieren entrega garantizada (Canal Reliable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    // Del cliente
    Join {
        player_name: String,
    },
    Ready,

    // Del servidor
    Welcome {
        player_id: u32,
        map: Option<Map>,
    },
    PlayerDisconnected {
        player_id: u32,
    },
    ChangeTeamColor {
        team_index: u8,
        color: (f32, f32, f32),
    },
    /// Dispara un movimiento predefinido en un jugador
    TriggerMovement {
        player_id: u32,
        movement_id: u8,
    },
    Error {
        message: String,
    },
}

/// Mensajes de alta frecuencia que toleran pérdida (Canal Unreliable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameDataMessage {
    // Del cliente
    Input {
        input: PlayerInput,
    },
    Ping {
        timestamp: u64,
    },

    // Del servidor
    GameState {
        tick: u32,
        timestamp: u64,
        players: Vec<PlayerState>,
        ball: BallState,
    },
    Pong {
        client_timestamp: u64,
        server_timestamp: u64,
    },
}

// ============================================================================
// MENSAJES ORIGINALES (Mantener por compatibilidad durante transición)
// ============================================================================

/// Mensajes que el cliente envía al servidor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Join {
        player_name: String,
        input_type: NetworkInputType,
    },
    Input {
        sequence: u32,
        input: PlayerInput,
    },
    Ping {
        timestamp: u64,
    },
    Ready,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum NetworkInputType {
    Keyboard,
    Gamepad,
    Touch,
}

/// Input del jugador completo
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct PlayerInput {
    pub move_up: bool,
    pub move_down: bool,
    pub move_left: bool,
    pub move_right: bool,
    pub kick: bool,
    pub curve_left: bool,
    pub curve_right: bool,
    pub stop_interact: bool,
    pub sprint: bool,
    pub dash: bool,
    pub mode: bool,
}

/// Mensajes que el servidor envía al cliente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        player_id: u32,
        map: Option<Map>,
    },

    GameState {
        tick: u32,
        timestamp: u64,
        players: Vec<PlayerState>,
        ball: BallState,
    },

    Pong {
        client_timestamp: u64,
        server_timestamp: u64,
    },

    PlayerDisconnected {
        player_id: u32,
    },

    ChangeTeamColor {
        team_index: u8,
        color: (f32, f32, f32),
    },

    /// Dispara un movimiento predefinido en un jugador
    TriggerMovement {
        player_id: u32,
        movement_id: u8,
    },

    Error {
        message: String,
    },
}

/// Movimiento activo de un jugador
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerMovement {
    pub movement_id: u8,
    pub start_tick: u32,
    pub end_tick: u32,
}

/// Estado completo de un jugador
#[derive(Serialize, Deserialize, Clone, Debug, Component)]
pub struct PlayerState {
    pub id: u32,
    pub name: String,
    pub position: Vec2,
    pub velocity: (f32, f32),
    pub rotation: f32,
    pub kick_charge: Vec2, // x = potencia, y = curva
    pub kick_charging: bool,
    pub is_sliding: bool,
    pub not_interacting: bool,
    pub ball_target_position: Option<Vec2>,
    pub stamin_charge: f32,
    // Movimiento visual activo
    pub active_movement: Option<PlayerMovement>,
    // Team
    pub team_index: u8,
    // Modo cubo activo
    pub mode_active: bool,
}

/// Estado de la pelota
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallState {
    pub position: (f32, f32),
    pub velocity: (f32, f32),
    pub angular_velocity: f32,
}

/// Configuración completa del juego (del código original)
#[derive(Debug, Clone, Serialize, Deserialize, bevy::prelude::Resource)]
pub struct GameConfig {
    // Colores de equipo (RGB)
    pub team_colors: Vec<(f32, f32, f32)>,

    // Velocidades básicas
    pub player_speed: f32,
    pub walk_coeficient: f32,
    pub run_coeficient: f32,
    pub kick_force: f32,
    pub attract_force: f32,
    pub magnus_coefficient: f32,

    // Propiedades físicas de las esferas
    pub sphere_radius: f32,
    pub ball_radius: f32,

    // Propiedades de materiales
    pub ball_friction: f32,
    pub ball_restitution: f32,
    pub ball_mass: f32,
    pub sphere_friction: f32,
    pub sphere_restitution: f32,

    // Propiedades de damping
    pub ball_linear_damping: f32,
    pub ball_angular_damping: f32,
    pub sphere_linear_damping: f32,
    pub sphere_angular_damping: f32,

    // Fuerzas y efectos
    pub spin_transfer: f32,
    pub max_control_offset: f32,
    pub attract_min_distance: f32,
    pub attract_max_distance: f32,

    // Arena
    pub arena_width: f32,
    pub arena_height: f32,
    pub wall_restitution: f32,

    // Dash time
    pub stamin: f32,
    pub dash_stamin_cost: f32,
    pub slide_stamin_cost: f32,
    pub run_stamin_coeficient_cost: f32,
    pub stamin_coeficient_restore: f32,

    //Slide
    pub speed_slide_coefficient: f32,
    pub slide_punch_force: f32,
    pub slide_max_torque: f32,

    // Map loading
    #[serde(default)]
    pub map_path: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            // Colores de equipo por defecto
            team_colors: vec![
                (0.9, 0.2, 0.2), // Equipo 0: Rojo
                (0.2, 0.4, 0.9), // Equipo 1: Azul
            ],

            // Velocidades básicas
            player_speed: 385.0,
            walk_coeficient: 0.70,
            run_coeficient: 1.0,
            kick_force: 2000000.0,
            attract_force: 800.0,
            magnus_coefficient: 33.0,

            // Propiedades físicas de las esferas
            sphere_radius: 45.0,
            ball_radius: 15.0,

            // Propiedades de materiales
            ball_friction: 0.3,
            ball_restitution: 0.6, // Más rebote (arcade)
            ball_mass: 0.1,        // Más liviana (arcade)
            sphere_friction: 0.8,
            sphere_restitution: 0.4,

            // Propiedades de damping
            ball_linear_damping: 1.5, // Rozamiento moderado
            ball_angular_damping: 0.5,
            sphere_linear_damping: 7.0,
            sphere_angular_damping: 5.0,

            // Fuerzas y efectos
            spin_transfer: 5.0,
            max_control_offset: 25.0,
            attract_min_distance: 35.0,
            attract_max_distance: 100.0,

            // Arena
            arena_width: 8000.0,
            arena_height: 4500.0,
            wall_restitution: 0.8,

            //Dash time
            stamin: 1.0,
            dash_stamin_cost: 0.02,
            slide_stamin_cost: 0.02,
            run_stamin_coeficient_cost: 0.02,
            stamin_coeficient_restore: 0.08,

            //Slide
            speed_slide_coefficient: 2.0,
            slide_punch_force: 300000.0,
            slide_max_torque: 1000.0,

            // Map loading
            map_path: None,
        }
    }
}
