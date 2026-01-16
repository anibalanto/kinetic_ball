// ============================================================================
// SISTEMAS DE FÍSICA DEL JUEGO
// ============================================================================

use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use shared::GameConfig;

use crate::input::GameAction;
use crate::{Ball, GameInputManager, Player, Sphere};

pub fn move_players(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut players: Query<&mut Player>,
    mut sphere_query: Query<&mut Velocity, With<Sphere>>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        // Si está en slide, no procesar input de movimiento
        if player.is_sliding {
            continue;
        }

        let sphere_entity = player.sphere;
        let player_id = player.id;

        if let Ok(mut velocity) = sphere_query.get_mut(sphere_entity) {
            let mut movement = Vec2::ZERO;

            // Movimiento usando GameInputManager (igual que RustBall)
            if game_input.is_pressed(player_id, GameAction::MoveUp) {
                movement.y += 1.0;
            }
            if game_input.is_pressed(player_id, GameAction::MoveDown) {
                movement.y -= 1.0;
            }
            if game_input.is_pressed(player_id, GameAction::MoveLeft) {
                movement.x -= 1.0;
            }
            if game_input.is_pressed(player_id, GameAction::MoveRight) {
                movement.x += 1.0;
            }

            if movement.length() > 0.0 {
                let run_tamin_cost = time.delta_seconds() * config.run_stamin_coeficient_cost;
                let move_coeficient = if game_input.is_pressed(player_id, GameAction::Sprint)
                    && player.stamin > run_tamin_cost
                {
                    player.stamin -= run_tamin_cost;
                    config.run_coeficient
                } else {
                    config.walk_coeficient
                };
                velocity.linvel =
                    movement.normalize_or_zero() * config.player_speed * move_coeficient;
            } else {
                velocity.linvel = Vec2::ZERO;
            }
        }
    }
}

// Sistema de RustBall - permite atravesar la pelota con Sprint
pub fn handle_collision_player(
    game_input: Res<GameInputManager>,
    mut player_query: Query<&mut Player>,
    mut sphere_query: Query<&mut SolverGroups, With<Sphere>>,
) {
    for mut player in player_query.iter_mut() {
        let player_id = player.id;

        let stop_interact = game_input.is_pressed(player_id, GameAction::StopInteract);
        player.not_interacting = stop_interact;

        if let Ok(mut solver_groups) = sphere_query.get_mut(player.sphere) {
            if game_input.is_pressed(player_id, GameAction::StopInteract) {
                // Con Sprint: no respuesta física con pelota (GROUP_3), sí con jugadores (GROUP_4) y paredes
                solver_groups.filters = Group::ALL ^ Group::GROUP_3;
            } else {
                // Normal: respuesta física con todos
                solver_groups.filters = Group::ALL;
            }
        }
    }
}

// Sistema de carga de patada
pub fn charge_kick(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut players: Query<&mut Player>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse, &mut Ball)>,
    sphere_query: Query<&Transform, With<Sphere>>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        let player_id = player.id;

        // Cualquiera de los 3 botones inicia la carga
        let kick_pressed = game_input.is_pressed(player_id, GameAction::Kick);
        let curve_left_pressed = game_input.is_pressed(player_id, GameAction::CurveLeft);
        let curve_right_pressed = game_input.is_pressed(player_id, GameAction::CurveRight);

        let any_kick_button = kick_pressed || curve_left_pressed || curve_right_pressed;
        let just_pressed = game_input.just_pressed(player_id, GameAction::Kick)
            || game_input.just_pressed(player_id, GameAction::CurveLeft)
            || game_input.just_pressed(player_id, GameAction::CurveRight);

        if let Ok(player_transform) = sphere_query.get(player.sphere) {
            for (ball_transform, mut impulse, mut ball) in ball_query.iter_mut() {
                let distance = player_transform
                    .translation
                    .distance(ball_transform.translation);

                if distance > config.kick_distance_threshold * 3.0 {
                    player.kick_charging = false;
                    player.kick_charge = 0.0;
                } else {
                    if just_pressed {
                        player.kick_charging = true;
                        player.kick_charge = 0.0;
                    }

                    if any_kick_button && player.kick_charging {
                        player.kick_charge += 2.0 * time.delta_seconds();
                        if player.kick_charge > 1.0 {
                            player.kick_charge = 1.0;
                        }
                    }
                }
            }
        }
    }
}

// SISTEMA DE KICK MEJORADO - Usa impulso en vez de reemplazar velocidad
pub fn kick_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse, &mut Ball)>,
    sphere_query: Query<&Transform, With<Sphere>>,
    mut player_query: Query<&mut Player>,
) {
    for mut player in player_query.iter_mut() {
        let player_id = player.id;

        let any_kick_button = game_input.is_pressed(player_id, GameAction::Kick)
            || game_input.is_pressed(player_id, GameAction::CurveLeft)
            || game_input.is_pressed(player_id, GameAction::CurveRight);

        let should_reset_kick = !any_kick_button && player.kick_charging;

        if should_reset_kick {
            player.kick_charging = false;

            if player.kick_charge > 0.0 {
                // Chequear si este jugador soltó algún botón de patada
                //let kick_released = game_input.just_released(player_id, GameAction::Kick);
                let curve_left_released =
                    game_input.just_released(player_id, GameAction::CurveLeft);
                let curve_right_released =
                    game_input.just_released(player_id, GameAction::CurveRight);

                // Determinar curva según qué botón soltaste
                let auto_curve = if curve_right_released {
                    -1.0
                } else if curve_left_released {
                    1.0
                } else {
                    0.0
                };

                if let Ok(player_transform) = sphere_query.get(player.sphere) {
                    for (ball_transform, mut impulse, mut ball) in ball_query.iter_mut() {
                        let distance = player_transform
                            .translation
                            .distance(ball_transform.translation);

                        if distance < config.kick_distance_threshold {
                            let mut direction = (ball_transform.translation
                                - player_transform.translation)
                                .truncate()
                                .normalize_or_zero();

                            // La curva es directamente auto_curve (según botón presionado)
                            let final_curve = auto_curve;

                            // Inclinación física de 30 grados
                            let angle_rad = 30.0f32.to_radians();
                            let tilt_angle = if final_curve > 0.0 {
                                -angle_rad
                            } else if final_curve < 0.0 {
                                angle_rad
                            } else {
                                0.0
                            };

                            if tilt_angle != 0.0 {
                                let (sin_a, cos_a) = tilt_angle.sin_cos();
                                direction = Vec2::new(
                                    direction.x * cos_a - direction.y * sin_a,
                                    direction.x * sin_a + direction.y * cos_a,
                                );
                            }

                            // Aplicamos el impulso de salida
                            impulse.impulse =
                                direction * (player.kick_charge * config.kick_force * 2000.0);

                            // Aplicamos el torque inicial (Spin)
                            let spin_force = final_curve * config.spin_transfer * 10.0;
                            impulse.torque_impulse = spin_force;
                            ball.angular_velocity = spin_force;
                        }
                    }
                }
            }
            // luego de hacer kick, pero en el bloque should_reset_kick
            player.kick_charge = 0.0;
        }
    }
}

pub fn look_at_ball(
    game_input: Res<GameInputManager>,
    player_query: Query<&Player>,
    mut sphere_query: Query<&mut Transform, With<Sphere>>,
    ball_query: Query<&Transform, (With<Ball>, Without<Sphere>)>,
) {
    if let Ok(ball_transform) = ball_query.get_single() {
        for player in player_query.iter() {
            // Durante slide, NO mirar la pelota - mantener rotación del deslizamiento
            if player.is_sliding {
                continue;
            }

            if let Ok(mut sphere_transform) = sphere_query.get_mut(player.sphere) {
                let direction =
                    (ball_transform.translation - sphere_transform.translation).truncate();

                if direction.length() > 0.0 {
                    let mut angle = direction.y.atan2(direction.x);

                    sphere_transform.rotation = Quat::from_rotation_z(angle);
                }
            }
        }
    }
}

pub fn apply_magnus_effect(
    config: Res<GameConfig>,
    mut ball_query: Query<(&mut ExternalForce, &Velocity, &mut Ball)>,
) {
    for (mut force, velocity, mut ball) in ball_query.iter_mut() {
        let speed = velocity.linvel.length();

        if speed > 5.0 && ball.angular_velocity.abs() > 0.1 {
            let velocity_dir = velocity.linvel.normalize_or_zero();
            let side_vector = Vec2::new(-velocity_dir.y, velocity_dir.x);

            // Igual que RustBall: multiplicar por velocidad
            let magnus_force_mag = config.magnus_coefficient * ball.angular_velocity * speed;
            force.force = side_vector * magnus_force_mag;

            // Decaimiento del spin por fricción del aire (igual que RustBall)
            ball.angular_velocity *= 0.98;
        } else {
            force.force = Vec2::ZERO;
            // NO resetear el spin - dejarlo decaer naturalmente
            // Solo aplicar decaimiento cuando hay spin
            if ball.angular_velocity.abs() > 0.01 {
                ball.angular_velocity *= 0.98;
            } else {
                ball.angular_velocity = 0.0;
            }
        }
    }
}

// SISTEMA DE ATRACCIÓN MEJORADO - Usa fuerza gradual en vez de reemplazar velocidad
pub fn attract_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    player_query: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), (With<Sphere>, Without<Ball>)>,
    mut ball_query: Query<
        (&Transform, &mut ExternalImpulse, &mut Velocity),
        (With<Ball>, Without<Sphere>),
    >,
) {
    for player in player_query.iter() {
        let player_id = player.id;

        // Con Sprint no hay interacción con la pelota
        if game_input.is_pressed(player_id, GameAction::Sprint)
            || game_input.is_pressed(player_id, GameAction::Kick)
        {
            continue;
        }

        if !game_input.is_pressed(player_id, GameAction::StopInteract) {
            if let Ok((player_transform, player_velocity)) = sphere_query.get(player.sphere) {
                for (ball_transform, mut impulse, mut velocity) in ball_query.iter_mut() {
                    let diff = player_transform.translation - ball_transform.translation;
                    let distance = diff.truncate().length();

                    if player_velocity.linvel.length()
                        > config.player_speed * (config.walk_coeficient + 0.1)
                    {
                        return;
                    }

                    // Radio de "pegado" - cuando está muy cerca, la pelota se queda pegada
                    let stick_radius = config.sphere_radius + 40.0;

                    if distance < stick_radius && distance > 1.0 {
                        // Efecto pegado: frenar la pelota y atraerla suavemente
                        let direction = diff.truncate().normalize_or_zero();

                        // Frenar la velocidad de la pelota (damping fuerte)
                        velocity.linvel *= 0.85;

                        // Atracción suave hacia el jugador
                        let stick_force = direction * 8000.0;
                        impulse.impulse += stick_force;
                    } else if distance < config.attract_max_distance
                        && distance > config.attract_min_distance
                    {
                        let direction = diff.truncate().normalize_or_zero();

                        // Fuerza de atracción que aumenta cuando la pelota se acerca
                        // pero no cuando ya está muy cerca (para evitar oscilaciones)
                        let distance_factor = 1.0
                            - (distance - config.attract_min_distance)
                                / (config.attract_max_distance - config.attract_min_distance);

                        // Reducir la fuerza si la pelota ya se mueve hacia el jugador
                        let current_velocity_toward_player = velocity.linvel.dot(direction);
                        let velocity_factor = if current_velocity_toward_player > 0.0 {
                            (1.0 - current_velocity_toward_player / 200.0).max(0.2)
                        } else {
                            1.0
                        };

                        let attract_impulse = direction
                            * config.attract_force
                            * distance_factor
                            * velocity_factor
                            * 0.016; // ~1/60 para frame
                        impulse.impulse += attract_impulse;
                    }
                }
            }
        }
    }
}

// Sistema de barrida: lee comando de slide del cliente y valida/ejecuta
pub fn detect_slide(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    time: Res<Time>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<(&Velocity, &Transform), With<Sphere>>,
) {
    for mut player in player_query.iter_mut() {
        let player_id = player.id;

        // Leer comando de slide desde el cliente
        if game_input.just_pressed(player_id, GameAction::Slide) {
            if config.slide_stamin_cost <= player.stamin && !player.is_sliding {
                // Obtener dirección actual del movimiento
                if let Ok((velocity, transform)) = sphere_query.get(player.sphere) {
                    let current_vel = velocity.linvel;

                    // Solo permitir slide si se está moviendo
                    if current_vel.length() > 50.0 {
                        player.is_sliding = true;
                        player.slide_timer = 0.3; // Duración de la barrida
                        let (_, _, angle) = transform.rotation.to_euler(EulerRot::XYZ);
                        player.slide_direction = Vec2::new(angle.cos(), angle.sin());
                        player.stamin -= config.slide_stamin_cost; // 1.5 segundos de cooldown

                        println!(
                            "🏃 Jugador {} inicia barrida hacia {:?}",
                            player_id, player.slide_direction
                        );
                    }
                }
            }
        }
    }
}

// Sistema de ejecución de barrida: aplica velocidad y cambia forma
pub fn execute_slide(
    config: Res<GameConfig>,
    time: Res<Time>,
    mut player_query: Query<&mut Player>,
    mut sphere_query: Query<(&mut Velocity, &mut Collider, &mut Transform), With<Sphere>>,
) {
    for mut player in player_query.iter_mut() {
        if player.is_sliding {
            if let Ok((mut velocity, mut collider, mut transform)) =
                sphere_query.get_mut(player.sphere)
            {
                // Aplicar velocidad fija en dirección del slide (doble de velocidad normal)
                let slide_speed = config.player_speed * 1.5;
                velocity.linvel = player.slide_direction * slide_speed;

                // Cambiar forma a cápsula orientada en dirección del movimiento
                // Calcular ángulo de la dirección (en radianes)
                let angle = player.slide_direction.y.atan2(player.slide_direction.x)
                    - std::f32::consts::FRAC_PI_2;

                // Rotar el Transform para que la cápsula vertical apunte en la dirección correcta
                transform.rotation = Quat::from_rotation_z(angle);

                // Cápsula vertical (en espacio local) de 45 (radio) + 15 de extensión
                let capsule_half_height = 15.0;
                *collider = Collider::capsule_y(capsule_half_height, config.sphere_radius);

                // Reducir timer
                player.slide_timer -= time.delta_seconds();

                // Si terminó la barrida
                if player.slide_timer <= 0.0 {
                    player.is_sliding = false;
                    // Restaurar forma original (esfera) y rotación
                    *collider = Collider::ball(config.sphere_radius);
                    transform.rotation = Quat::IDENTITY;
                    println!("🏁 Jugador {} termina barrida", player.id);
                }
            }
        }
    }
}

pub fn dash_first_touch_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<(&Transform, &Velocity), (With<Sphere>, Without<Ball>)>,
    mut ball_query: Query<(&Transform, &mut Velocity), With<Ball>>,
    time: Res<Time>,
) {
    let player_diameter = config.sphere_radius * 2.0;
    let target_distance = player_diameter * 1.5;
    let activation_radius = config.sphere_radius + config.ball_radius + 50.0;

    for mut player in player_query.iter_mut() {
        if game_input.is_pressed(player.id, GameAction::Dash) {
            if config.dash_stamin_cost <= player.stamin {
                if let Ok((player_transform, player_velocity)) = sphere_query.get(player.sphere) {
                    for (ball_transform, mut ball_velocity) in ball_query.iter_mut() {
                        let p_pos = player_transform.translation.truncate();
                        let b_pos = ball_transform.translation.truncate();
                        let diff = b_pos - p_pos;

                        let p_vel = if player_velocity.linvel.length_squared() < 0.1 {
                            // Vector desde el jugador hacia la pelota
                            let dir_to_ball = diff.normalize_or_zero();
                            // Asignamos una velocidad virtual (puedes usar config.player_speed o un valor fijo)
                            dir_to_ball * config.player_speed * 0.5
                        } else {
                            player_velocity.linvel
                        };

                        let p_dir = p_vel.normalize_or_zero();

                        if diff.length() < activation_radius {
                            // 1. POSICIÓN OBJETIVO BASE (Relativa al jugador ahora)
                            let base_target_pos = p_pos + (p_dir * target_distance);

                            // 2. PREDICCIÓN: ¿Dónde estará ese punto en 'T' segundos?
                            // Si el jugador se mueve a p_vel, el punto objetivo también.
                            let time_to_reach = 0.2; // Ajusta esto: 1.0 es lento, 0.2 es muy rápido
                            let predicted_target_pos = base_target_pos + (p_vel * time_to_reach);

                            player.ball_target_position = Some(predicted_target_pos);

                            // 3. CÁLCULO DE LA "VELOCIDAD JUSTA" PARA LLEGAR EN EL TIEMPO 'T'
                            let displacement = predicted_target_pos - b_pos;
                            let distance = displacement.length();

                            // v = d / t (Velocidad necesaria para cubrir la distancia en el tiempo deseado)
                            let required_speed = distance / time_to_reach;

                            // 4. DIRECCIÓN Y VELOCIDAD FINAL
                            // Importante: No sumamos p_vel aquí porque ya está implícito en la predicción
                            let target_velocity = displacement.normalize_or_zero() * required_speed;

                            // 5. APLICACIÓN FÍSICA (Suavizado para evitar latigazos)
                            // DeltaV = lo que quiero - lo que tengo
                            let delta_v = target_velocity - ball_velocity.linvel;

                            // Usamos un factor de respuesta. 1.0 es instantáneo, 0.5 es más elástico.
                            let responsiveness = 0.6;
                            ball_velocity.linvel += delta_v * responsiveness;

                            // 6. SEGURIDAD: Si está muy cerca, simplemente igualar velocidad
                            if distance < 2.0 {
                                ball_velocity.linvel = p_vel;
                            }

                            player.stamin -= config.dash_stamin_cost;
                            println!(
                                "⚡ Sprint Touch ejecutado. Cooldown iniciado para jugador {}",
                                player.id
                            );
                        }
                    }
                }
            }
        }
    }
}

pub fn update_ball_damping(
    config: Res<GameConfig>,
    mut ball_query: Query<(&mut Damping, &Velocity), With<Ball>>,
) {
    for (mut damping, velocity) in ball_query.iter_mut() {
        let speed = velocity.linvel.length();

        if speed < 50.0 {
            damping.linear_damping = config.ball_linear_damping * 3.0;
        } else {
            damping.linear_damping = config.ball_linear_damping;
        }
    }
}

pub fn recover_stamin(
    config: Res<GameConfig>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<&Velocity, With<Sphere>>,
    time: Res<Time>,
) {
    for mut player in player_query.iter_mut() {
        if let Ok(velocity) = sphere_query.get(player.sphere) {
            if player.stamin > 1.0 {
                player.stamin = 1.0;
            } else if player.stamin < 1.0 {
                let speed = velocity.linvel.length();
                if speed <= config.player_speed * config.walk_coeficient {
                    player.stamin += time.delta_seconds() * config.run_stamin_coeficient_cost * 2.0;
                }
            }
        }
    }
}
