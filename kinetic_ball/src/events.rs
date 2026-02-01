use bevy::prelude::*;

use crate::shared::protocol::PlayerState;

/// Evento para solicitar el spawning visual de la pelota
#[derive(Message)]
pub struct SpawnBallEvent {
    pub position: (f32, f32),
    pub velocity: (f32, f32),
}

/// Evento para solicitar el spawning visual de un jugador
#[derive(Message)]
pub struct SpawnPlayerEvent {
    pub player_state: PlayerState,
    pub is_local: bool,
}
