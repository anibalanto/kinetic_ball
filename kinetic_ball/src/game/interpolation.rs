use bevy::prelude::*;

use crate::components::{CurveAction, Interpolated, KeyVisual, RemotePlayer, SlideCubeVisual};
use crate::keybindings::{GamepadBindingsMap, GilrsWrapper, KeyBindingsConfig, RawGamepadInput};
use crate::local_players::{idx_to_gilrs_axis, InputDevice, LocalPlayers};
use crate::resources::GameTick;
use crate::shared::movements::{get_movement, AnimatedProperty};
use crate::shared::protocol::GameConfig;

// 3. Sistema de interpolación (Actualizado)
pub fn interpolate_entities(time: Res<Time>, mut q: Query<(&mut Transform, &Interpolated)>) {
    let dt = time.delta_secs();
    for (mut transform, interp) in q.iter_mut() {
        // Interpolar posición
        let prediction_offset = interp.target_velocity * dt;
        let effective_target = interp.target_position + prediction_offset;
        let current_pos = transform.translation.truncate();
        let new_pos = current_pos.lerp(effective_target, dt * interp.smoothing);
        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;

        // Interpolar rotación
        let (_, _, current_rotation) = transform.rotation.to_euler(EulerRot::XYZ);
        let rotation_diff = interp.target_rotation - current_rotation;

        // Normalizar el ángulo para tomar el camino más corto
        let rotation_diff = if rotation_diff > std::f32::consts::PI {
            rotation_diff - 2.0 * std::f32::consts::PI
        } else if rotation_diff < -std::f32::consts::PI {
            rotation_diff + 2.0 * std::f32::consts::PI
        } else {
            rotation_diff
        };

        let new_rotation = current_rotation + rotation_diff * (dt * interp.smoothing);
        transform.rotation = Quat::from_rotation_z(new_rotation);
    }
}

// Sistema para procesar movimientos activos y actualizar el cubo de dirección
pub fn process_movements(
    game_tick: Res<GameTick>,
    player_query: Query<(&RemotePlayer, &Children)>,
    mut cube_query: Query<(&SlideCubeVisual, &mut Transform)>,
    config: Res<GameConfig>,
) {
    let current_tick = game_tick.0;

    for (player, children) in player_query.iter() {
        // Obtener el movimiento activo del jugador (si existe)
        let Some(ref active_movement) = player.active_movement else {
            continue;
        };

        // Calcular progreso basado en ticks
        let start = active_movement.start_tick;
        let end = active_movement.end_tick;

        // Si ya pasó el end_tick, el movimiento terminó
        if current_tick >= end {
            continue;
        }

        // Si aún no llegamos al start_tick, no ejecutar
        if current_tick < start {
            continue;
        }

        // Calcular progreso (0.0 a 1.0)
        let duration = (end - start) as f32;
        let elapsed = (current_tick - start) as f32;
        let progress = (elapsed / duration).clamp(0.0, 1.0);

        // Obtener el movimiento desde el catálogo compartido
        let Some(movement) = get_movement(active_movement.movement_id) else {
            continue;
        };

        // Buscar el cubo hijo de este jugador
        for child in children.iter() {
            if let Ok((cube_visual, mut cube_transform)) = cube_query.get_mut(child) {
                if cube_visual.parent_id != player.id {
                    continue;
                }

                // Evaluar cada propiedad animada usando keyframes
                // Scale
                if let Some(scale) = movement.evaluate(AnimatedProperty::Scale, progress) {
                    cube_transform.scale = Vec3::splat(scale);
                }

                // OffsetX (multiplicador del radio)
                if let Some(offset_mult) = movement.evaluate(AnimatedProperty::OffsetX, progress) {
                    cube_transform.translation.x = config.sphere_radius * offset_mult;
                }

                // OffsetY (multiplicador del radio)
                if let Some(offset_mult) = movement.evaluate(AnimatedProperty::OffsetY, progress) {
                    cube_transform.translation.y = config.sphere_radius * offset_mult;
                }

                // Rotación adicional (se suma a la base de 45°)
                if let Some(rotation) = movement.evaluate(AnimatedProperty::Rotation, progress) {
                    cube_transform.rotation =
                        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4 + rotation);
                }
            }
        }
    }
}

pub fn animate_keys(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    key_query: Query<(&KeyVisual, &Children)>,
    mut transform_query: Query<&mut Transform>,
    time: Res<Time>,
    local_players: Res<LocalPlayers>,
    keybindings: Res<KeyBindingsConfig>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    gilrs: Option<Res<GilrsWrapper>>,
) {
    // Pre-cargar el estado de gilrs si está disponible
    let gilrs_guard = gilrs.as_ref().and_then(|g| g.gilrs.lock().ok());

    for (key_visual, children) in key_query.iter() {
        // El cuerpo móvil es el segundo hijo (índice 1) según nuestro spawn_key_visual_2d
        if let Some(&body_entity) = children.get(1) {
            if let Ok(mut transform) = transform_query.get_mut(body_entity) {
                // Buscar el jugador local correspondiente
                let local_player = local_players
                    .players
                    .iter()
                    .find(|lp| lp.server_player_id == Some(key_visual.player_id));

                // Determinar si el botón está presionado
                let is_pressed = if let Some(lp) = local_player {
                    match &lp.input_device {
                        InputDevice::Keyboard => {
                            // Usar keybindings de teclado
                            let key_code = match key_visual.action {
                                CurveAction::Left => keybindings.curve_left.0,
                                CurveAction::Right => keybindings.curve_right.0,
                            };
                            keyboard_input.pressed(key_code)
                        }
                        InputDevice::RawGamepad(gamepad_id) => {
                            // Usar bindings del gamepad
                            if let Some(ref gilrs_instance) = gilrs_guard {
                                if let Some(gamepad) = gilrs_instance.connected_gamepad(*gamepad_id)
                                {
                                    let bindings = lp
                                        .gamepad_type_name
                                        .as_ref()
                                        .map(|name| gamepad_bindings_map.get_bindings(name))
                                        .unwrap_or_default();

                                    let binding = match key_visual.action {
                                        CurveAction::Left => &bindings.curve_left,
                                        CurveAction::Right => &bindings.curve_right,
                                    };

                                    is_gamepad_binding_active(gamepad, binding)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                } else {
                    false
                };

                // Si el botón está presionado, el objetivo es -4.0 (hundida hacia la sombra)
                // Si no, el objetivo es 0.0 (posición original)
                let target_y = if is_pressed { -4.0 } else { 0.0 };

                // Usamos un lerp suave para que la tecla no "teletransporte",
                // sino que se sienta elástica y física.
                let speed = 25.0;
                transform.translation.y = transform.translation.y
                    + (target_y - transform.translation.y) * speed * time.delta_secs();
            }
        }
    }
}

/// Helper para verificar si un binding de gamepad está activo
pub fn is_gamepad_binding_active(
    gamepad: gilrs::Gamepad<'_>,
    binding: &Option<RawGamepadInput>,
) -> bool {
    if let Some(b) = binding {
        match b {
            RawGamepadInput::Button(idx) => {
                // Verificar si el botón está presionado
                for (code, data) in gamepad.state().buttons() {
                    if data.is_pressed() {
                        let raw_code: u32 = code.into_u32();
                        let button_idx = if raw_code >= 288 && raw_code < 320 {
                            (raw_code - 288) as u8
                        } else if raw_code >= 304 && raw_code < 320 {
                            (raw_code - 304) as u8
                        } else {
                            (raw_code & 0x1F) as u8
                        };
                        if button_idx == *idx {
                            return true;
                        }
                    }
                }
                false
            }
            RawGamepadInput::AxisPositive(idx) => {
                idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) > 0.5)
            }
            RawGamepadInput::AxisNegative(idx) => {
                idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) < -0.5)
            }
        }
    } else {
        false
    }
}
