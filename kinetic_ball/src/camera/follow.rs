use bevy::prelude::*;

use crate::components::{
    MinimapCamera, PlayerCamera, PlayerDetailCamera, RemoteBall, RemotePlayer,
};
use crate::local_players::LocalPlayers;
use crate::resources::{DynamicSplitState, MyPlayerId};

pub fn camera_follow_player_and_ball(
    my_player_id: Res<MyPlayerId>,
    local_players: Res<LocalPlayers>,
    split_state: Res<DynamicSplitState>,
    _ball_query: Query<
        &Transform,
        (
            With<RemoteBall>,
            Without<PlayerCamera>,
            Without<MinimapCamera>,
            Without<PlayerDetailCamera>,
        ),
    >,
    players: Query<
        (&RemotePlayer, &Transform),
        (
            Without<PlayerCamera>,
            Without<MinimapCamera>,
            Without<PlayerDetailCamera>,
        ),
    >,
    mut cameras: ParamSet<(
        Query<(&mut Transform, &PlayerCamera)>,
        Query<&mut Transform, With<MinimapCamera>>,
        Query<(&mut Transform, &PlayerDetailCamera)>,
    )>,
    time: Res<Time>,
    windows: Query<&Window>,
) {
    let Some(_) = windows.iter().next() else {
        return;
    };

    let delta = time.delta_secs();
    let smoothing = 10.0;

    // Calcular cuántos jugadores locales hay
    let num_local_players = local_players.players.len().max(1);
    let _use_split = num_local_players > 1;

    // Recopilar posiciones de todos los jugadores locales para calcular centroide
    let mut local_player_positions: Vec<Vec3> = Vec::new();
    for local_player in local_players.players.iter().take(2) {
        if let Some(server_id) = local_player.server_player_id {
            if let Some((_, transform)) = players.iter().find(|(p, _)| p.id == server_id) {
                local_player_positions.push(transform.translation);
            }
        }
    }

    // Calcular centroide de todos los jugadores locales
    let centroid = if local_player_positions.is_empty() {
        None
    } else {
        let sum: Vec3 = local_player_positions.iter().copied().sum();
        Some(sum / local_player_positions.len() as f32)
    };

    // Factor de split: 0 = unified (seguir centroide), 1 = split (cada cámara sigue su jugador)
    let split_factor = split_state.split_factor;
    let split_angle = split_state.split_angle;

    // Calcular el vector normal del split (dirección entre jugadores)
    // Este es el eje perpendicular a la línea de división
    let split_normal = Vec2::new(split_angle.cos(), split_angle.sin());

    // Iterar sobre todas las cámaras de jugador
    for (mut cam_transform, player_camera) in cameras.p0().iter_mut() {
        // Determinar qué jugador debe seguir esta cámara
        let target_player_id = player_camera.server_player_id.or(my_player_id.0);

        let Some(target_id) = target_player_id else {
            continue;
        };

        // Buscar la posición del jugador objetivo
        let player_pos = players
            .iter()
            .find(|(p, _)| p.id == target_id)
            .map(|(_, t)| t.translation);

        if let Some(p_pos) = player_pos {
            // Centroide de los jugadores
            let unified_target = centroid.unwrap_or(p_pos);

            // Cuando split_factor > 0, calcular el centro de la región de esta cámara
            //
            // La pantalla se divide con una línea. Cada mitad es un cuadrilátero.
            // El centro del cuadrilátero es la intersección de sus diagonales.
            //
            // Para simplificar: el centro de cada mitad está aproximadamente a 1/4
            // del viewport desde el centro, EN LA DIRECCIÓN del jugador respecto al centro.
            //
            // En vez de usar el split_normal (que apunta entre jugadores),
            // calculamos el offset basado en la posición del jugador respecto al centroide.

            let split_target = if let Some(cent) = centroid {
                // Vector desde este jugador hacia el centroide (hacia el OTRO jugador)
                let to_center = cent - p_pos;
                let distance_to_center = to_center.truncate().length();

                if distance_to_center > 1.0 {
                    // Para que el jugador aparezca en el centro de SU mitad de pantalla:
                    // - El centro de su mitad está DESPLAZADO del centro de pantalla
                    // - El desplazamiento es HACIA su lado (alejándose del otro jugador)
                    // - Para lograr esto, la cámara debe moverse HACIA el otro jugador
                    //
                    // Magnitud: ~1/4 del viewport visible
                    let visible_quarter = 240.0 * 3.0; // ~720 unidades a scale 3.0

                    // Dirección: hacia el centroide (hacia el otro jugador)
                    let dir = to_center.truncate().normalize();

                    // Mover cámara HACIA el otro jugador
                    // Esto hace que el jugador aparezca desplazado hacia SU lado de la pantalla
                    let camera_offset =
                        Vec3::new(dir.x * visible_quarter, dir.y * visible_quarter, 0.0);

                    p_pos + camera_offset
                } else {
                    p_pos
                }
            } else {
                p_pos
            };

            // Interpolar entre unified (centroide) y split (jugador en centro de su región)
            let final_target = unified_target.lerp(split_target, split_factor);

            // Aplicar movimiento suavizado
            cam_transform.translation.x +=
                (final_target.x - cam_transform.translation.x) * smoothing * delta;
            cam_transform.translation.y +=
                (final_target.y - cam_transform.translation.y) * smoothing * delta;
        }
    }

    // 5. Cámaras de Detalle (cada una sigue a su jugador local correspondiente)
    for (mut cam_transform, detail_camera) in cameras.p2().iter_mut() {
        // Buscar el jugador local correspondiente a esta cámara de detalle
        let target_player_id = local_players
            .players
            .get(detail_camera.local_index as usize)
            .and_then(|lp| lp.server_player_id)
            .or_else(|| {
                // Fallback: si no hay jugador local, usar my_player_id para la primera cámara
                if detail_camera.local_index == 0 {
                    my_player_id.0
                } else {
                    None
                }
            });

        if let Some(target_id) = target_player_id {
            if let Some(p_pos) = players
                .iter()
                .find(|(p, _)| p.id == target_id)
                .map(|(_, t)| t.translation)
            {
                cam_transform.translation.x = p_pos.x;
                cam_transform.translation.y = p_pos.y;
            }
        }
    }
}

// Sistema de control de zoom con teclas numéricas
pub fn camera_zoom_control(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut cameras: Query<&mut Projection, With<PlayerCamera>>,
) {
    let mut new_scale = None;

    // Teclas 1-9 para diferentes niveles de zoom
    if keyboard.just_pressed(KeyCode::Digit9) {
        new_scale = Some(0.5); // Muy cerca
    } else if keyboard.just_pressed(KeyCode::Digit8) {
        new_scale = Some(0.75);
    } else if keyboard.just_pressed(KeyCode::Digit7) {
        new_scale = Some(1.0); // Normal
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        new_scale = Some(1.3);
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        new_scale = Some(1.5);
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        new_scale = Some(2.0); // Lejos
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        new_scale = Some(2.5);
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        new_scale = Some(3.0);
    } else if keyboard.just_pressed(KeyCode::Digit1) {
        new_scale = Some(4.0); // Muy lejos
    }

    if let Some(scale) = new_scale {
        // Aplicar zoom a todas las cámaras de jugador
        for mut projection_comp in cameras.iter_mut() {
            if let Projection::Orthographic(ref mut projection) = projection_comp.as_mut() {
                projection.scale = scale;
            }
        }
        println!("📷 Zoom ajustado a: {:.1}x", scale);
    }
}
