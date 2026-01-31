use bevy::prelude::*;

use crate::components::{PlayerCamera, RemotePlayer, SplitScreenQuad};
use crate::local_players::LocalPlayers;
use crate::resources::{DynamicSplitState, SplitMode, SplitScreenMaterial};

/// Sistema que sincroniza los server_player_id de las cámaras con LocalPlayers
/// (Ya no maneja viewports porque usamos render-to-texture para split dinámico)
pub fn update_camera_viewports(
    local_players: Res<LocalPlayers>,
    mut cameras: Query<&mut PlayerCamera>,
) {
    for mut player_cam in cameras.iter_mut() {
        // Sincronizar server_player_id desde LocalPlayers
        if let Some(local_player) = local_players.players.get(player_cam.local_index as usize) {
            player_cam.server_player_id = local_player.server_player_id;
        }
    }
}

/// Calcula el ángulo del vector entre jugadores.
/// El shader usa este ángulo como normal - la línea de corte será perpendicular.
/// Nota: Las coordenadas UV tienen Y invertido, por eso negamos direction.y
pub fn calculate_split_angle(player1_pos: Vec2, player2_pos: Vec2) -> f32 {
    let direction = player2_pos - player1_pos;
    // El ángulo es la dirección entre jugadores (el shader dibuja la línea perpendicular)
    // Negamos Y porque las UV tienen Y invertido respecto al mundo
    (-direction.y).atan2(direction.x)
}

/// Sistema que actualiza el estado del split-screen dinámico
pub fn update_split_screen_state(
    local_players: Res<LocalPlayers>,
    players: Query<(&RemotePlayer, &Transform)>,
    cameras: Query<&Projection, With<PlayerCamera>>,
    mut split_state: ResMut<DynamicSplitState>,
    time: Res<Time>,
) {
    // Solo procesar si hay 2+ jugadores locales
    if local_players.players.len() < 2 {
        split_state.mode = SplitMode::Unified;
        split_state.split_factor = 0.0;
        return;
    }

    // Obtener posiciones de los jugadores locales
    let mut local_positions: Vec<Vec2> = Vec::new();

    for local_player in local_players.players.iter().take(2) {
        if let Some(server_id) = local_player.server_player_id {
            if let Some((_, transform)) = players.iter().find(|(p, _)| p.id == server_id) {
                local_positions.push(transform.translation.truncate());
            }
        }
    }

    // Necesitamos al menos 2 posiciones
    if local_positions.len() < 2 {
        return;
    }

    let pos1 = local_positions[0];
    let pos2 = local_positions[1];

    // Calcular distancia entre jugadores
    let distance = pos1.distance(pos2);

    // Obtener escala de zoom para ajustar umbrales
    let zoom_scale = cameras
        .iter()
        .next()
        .and_then(|p| {
            if let Projection::Orthographic(ortho) = p {
                Some(ortho.scale)
            } else {
                None
            }
        })
        .unwrap_or(3.0);

    // Calcular umbral visible basado en zoom (aproximación del viewport visible)
    // A mayor zoom (scale), menos se ve, así que los umbrales deben ser menores
    let base_visible = 600.0; // Distancia base visible a zoom 1.0
    let visible_distance = base_visible * zoom_scale;

    // Umbrales con histéresis para evitar parpadeo
    let split_threshold = visible_distance * 0.8; // 80% del visible para separar
    let merge_threshold = visible_distance * 0.5; // 50% del visible para fusionar

    split_state.split_threshold = split_threshold;
    split_state.merge_threshold = merge_threshold;
    split_state.viewport_visible_ratio = zoom_scale;

    // Determinar modo objetivo
    let target_mode = match split_state.mode {
        SplitMode::Unified => {
            if distance > split_threshold {
                SplitMode::Split
            } else {
                SplitMode::Unified
            }
        }
        SplitMode::Split => {
            if distance < merge_threshold {
                SplitMode::Unified
            } else {
                SplitMode::Split
            }
        }
        SplitMode::Transitioning => {
            // Durante transición, usar umbrales sin histéresis
            if distance > (split_threshold + merge_threshold) / 2.0 {
                SplitMode::Split
            } else {
                SplitMode::Unified
            }
        }
    };

    // Calcular factor objetivo
    let target_factor = match target_mode {
        SplitMode::Unified => 0.0,
        SplitMode::Split => 1.0,
        SplitMode::Transitioning => split_state.split_factor,
    };

    // Interpolación suave del factor
    let transition_speed = 5.0;
    let t = time.delta_secs() * transition_speed;
    let new_factor = split_state.split_factor + (target_factor - split_state.split_factor) * t;

    // Actualizar modo basado en el factor
    split_state.mode = if new_factor < 0.01 {
        SplitMode::Unified
    } else if new_factor > 0.99 {
        SplitMode::Split
    } else {
        SplitMode::Transitioning
    };

    split_state.split_factor = new_factor.clamp(0.0, 1.0);

    // Calcular ángulo de división
    split_state.split_angle = calculate_split_angle(pos1, pos2);
}

/// Sistema que actualiza el material del compositor con los parámetros actuales
pub fn update_split_compositor(
    split_state: Res<DynamicSplitState>,
    split_quad: Query<&MeshMaterial2d<SplitScreenMaterial>, With<SplitScreenQuad>>,
    mut split_materials: ResMut<Assets<SplitScreenMaterial>>,
) {
    for material_handle in split_quad.iter() {
        if let Some(material) = split_materials.get_mut(material_handle) {
            // Actualizar los parámetros del shader
            // x: angle, y: factor, z: center_x, w: center_y
            material.split_params = Vec4::new(
                split_state.split_angle,
                split_state.split_factor,
                0.5, // Centro X (normalizado 0-1)
                0.5, // Centro Y (normalizado 0-1)
            );
        }
    }
}
