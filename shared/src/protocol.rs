use serde::{Deserialize, Serialize};
use bevy::prelude::{Vec2, Component};

/// Mensajes que el cliente envía al servidor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Join { player_name: String, input_type: NetworkInputType },
    Input { sequence: u32, input: PlayerInput },
    Ping { timestamp: u64 },
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
}

/// Mensajes que el servidor envía al cliente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        player_id: u32,
        game_config: GameConfig,
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
        player_id: u32
    },

    Error {
        message: String
    },
}

/// Estado completo de un jugador
#[derive(Serialize, Deserialize, Clone, Debug, Component)]
pub struct PlayerState {
    pub id: u32,
    pub name: String,
    pub position: Vec2,
    pub velocity: (f32, f32),
    pub rotation: f32,
    pub kick_charge: f32,
    pub kick_charging: bool,
    pub curve_charge: f32,
    pub curve_charging: bool,
}

/// Estado de la pelota
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallState {
    pub position: (f32, f32),
    pub velocity: (f32, f32),
    pub angular_velocity: f32,
}

/// Configuración completa del juego (del código original)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct GameConfig {
    // Velocidades básicas
    pub player_speed: f32,
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
    pub kick_distance_threshold: f32,
    pub attract_min_distance: f32,
    pub attract_max_distance: f32,

    // Arena
    pub arena_width: f32,
    pub arena_height: f32,
    pub wall_restitution: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            // Velocidades básicas
            player_speed: 350.0,
            kick_force: 800.0,
            attract_force: 800.0,
            magnus_coefficient: 33.0,

            // Propiedades físicas de las esferas
            sphere_radius: 45.0,
            ball_radius: 15.0,

            // Propiedades de materiales
            ball_friction: 0.7,
            ball_restitution: 0.4,
            ball_mass: 0.3,
            sphere_friction: 0.8,
            sphere_restitution: 0.4,

            // Propiedades de damping
            ball_linear_damping: 1.2,
            ball_angular_damping: 0.5,
            sphere_linear_damping: 8.0,
            sphere_angular_damping: 5.0,

            // Fuerzas y efectos
            spin_transfer: 5.0,
            max_control_offset: 25.0,
            kick_distance_threshold: 80.0,
            attract_min_distance: 35.0,
            attract_max_distance: 100.0,

            // Arena
            arena_width: 2000.0,
            arena_height: 1500.0,
            wall_restitution: 0.8,
        }
    }
}
