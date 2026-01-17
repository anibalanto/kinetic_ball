// ============================================================================
// SISTEMAS DE FÍSICA DEL JUEGO
// ============================================================================

use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use matchbox_socket::PeerId;
use shared::movements::{get_movement, movement_ids};
use shared::protocol::PlayerMovement;
use shared::{GameConfig, TICK_RATE};

use crate::input::GameAction;
use crate::{Ball, GameInputManager, GameTick, Player, SlideCube, Sphere};

pub fn spawn_physics(
    commands: &mut Commands,
    id: u32,
    name: String,
    peer_id: PeerId,
    config: &Res<GameConfig>,
) {
    // Spawn física del jugador (Sphere) - igual estructura que RustBall
    let spawn_x = ((id % 3) as f32 - 1.0) * 200.0;
    let spawn_y = ((id / 3) as f32 - 1.0) * 200.0;

    let sphere_entity = commands
        .spawn((
            Sphere,
            TransformBundle::from_transform(Transform::from_xyz(spawn_x, spawn_y, 0.0)),
            RigidBody::Dynamic,
            Collider::ball(config.sphere_radius),
            Velocity::zero(),
            // Jugador: colisiona con todo EXCEPTO líneas solo-pelota (GROUP_5)
            CollisionGroups::new(Group::GROUP_4, Group::ALL ^ Group::GROUP_5),
            SolverGroups::new(Group::GROUP_4, Group::ALL ^ Group::GROUP_5),
            Friction {
                coefficient: config.sphere_friction,
                combine_rule: CoefficientCombineRule::Min,
            },
            Restitution {
                coefficient: config.sphere_restitution,
                combine_rule: CoefficientCombineRule::Average,
            },
            Damping {
                linear_damping: config.sphere_linear_damping,
                angular_damping: config.sphere_angular_damping,
            },
            ExternalImpulse::default(),
            ExternalForce::default(),
        ))
        .id();

    // Spawn del cubo de dirección/slide (inicialmente sin física)
    let cube_size = config.sphere_radius / 3.0;
    let cube_offset = Vec2::new(config.sphere_radius * 0.7, 0.0);

    let slide_cube_entity = commands
        .spawn((
            SlideCube { owner_id: id },
            TransformBundle::from_transform(
                Transform::from_xyz(spawn_x + cube_offset.x, spawn_y + cube_offset.y, 0.0)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
                    .with_scale(Vec3::splat(1.0)),
            ),
        ))
        .id();

    // Asignar equipo basado en ID (par = 0, impar = 1)
    let team_index = (id % 2) as u8;

    // Spawn lógica del jugador (Player) - Usando peer_id ahora
    commands.spawn(Player {
        sphere: sphere_entity,
        slide_cube: slide_cube_entity,
        id,
        name: name.clone(),
        kick_charge: 0.0,
        kick_charging: false,
        peer_id,
        is_ready: false,
        not_interacting: false,
        is_sliding: false,
        slide_direction: Vec2::ZERO,
        slide_timer: 0.0,
        ball_target_position: None,
        stamin: 1.0,
        slide_cube_active: false,
        slide_cube_offset: cube_offset,
        slide_cube_scale: 1.0,
        active_movement: None,
        team_index,
    });

    println!("✅ Jugador {} spawneado: {}", id, name);
}

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
    tick: Res<GameTick>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<(&Velocity, &Transform), With<Sphere>>,
) {
    for mut player in player_query.iter_mut() {
        if game_input.just_pressed(player.id, GameAction::Slide) {
            if config.slide_stamin_cost <= player.stamin && !player.is_sliding {
                if let Ok((velocity, transform)) = sphere_query.get(player.sphere) {
                    if velocity.linvel.length() > 50.0 {
                        player.is_sliding = true;
                        player.slide_timer = 0.3; // Duración total

                        // DIRECCIÓN: Usamos la rotación actual (que mira a la pelota gracias a look_at_ball)
                        let (_, _, angle) = transform.rotation.to_euler(EulerRot::XYZ);
                        player.slide_direction = Vec2::new(angle.cos(), angle.sin());

                        player.stamin -= config.slide_stamin_cost;
                        player.slide_cube_active = true;

                        // Activar movimiento visual
                        if let Some(movement) = get_movement(movement_ids::SLIDE_CUBE_GROW) {
                            let duration_ticks = (movement.duration * TICK_RATE as f32) as u32;
                            player.active_movement = Some(PlayerMovement {
                                movement_id: movement_ids::SLIDE_CUBE_GROW,
                                start_tick: tick.0,
                                end_tick: tick.0 + duration_ticks,
                            });
                        }
                    }
                }
            }
        }
    }
}

pub fn execute_slide(
    mut commands: Commands,
    config: Res<GameConfig>,
    time: Res<Time>,
    tick: Res<GameTick>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<&Transform, (With<Sphere>, Without<SlideCube>)>,
    mut cube_query: Query<
        (Entity, &mut Transform, Option<&Collider>),
        (With<SlideCube>, Without<Sphere>),
    >,
) {
    for mut player in player_query.iter_mut() {
        if !player.is_sliding {
            continue;
        }

        if let Ok(sphere_transform) = sphere_query.get(player.sphere) {
            let total_time = 0.3;
            let elapsed = total_time - player.slide_timer;
            let progress = (elapsed / total_time).clamp(0.0, 1.0);

            // FASES: 1. Avanza (0-0.4), 2. Mantiene (0.4-0.7), 3. Retrocede (0.7-1.0)
            let (scale_factor, dist_factor) = if progress < 0.4 {
                let p = progress / 0.4;
                (p * 3.0, p)
            } else if progress < 0.7 {
                (3.0, 1.0)
            } else {
                let p = (progress - 0.7) / 0.3;
                (3.0 * (1.0 - p), 1.0 - p) // Vuelve hacia el jugador
            };

            player.slide_cube_scale = scale_factor.max(0.1);
            let max_dist = config.sphere_radius * 1.8; // Un poco más lejos para llegar bien
            player.slide_cube_offset = player.slide_direction * (max_dist * dist_factor);

            if let Ok((cube_entity, mut cube_transform, maybe_collider)) =
                cube_query.get_mut(player.slide_cube)
            {
                // POSICIÓN MUNDIAL: Posición jugador + Offset
                let player_pos = sphere_transform.translation.truncate();
                cube_transform.translation = (player_pos + player.slide_cube_offset).extend(2.0);
                cube_transform.scale = Vec3::splat(player.slide_cube_scale);

                if maybe_collider.is_none() && progress > 0.1 {
                    let size = (config.sphere_radius / 1.5) * player.slide_cube_scale;
                    commands.entity(cube_entity).insert((
                        Collider::cuboid(size / 2.0, size / 2.0),
                        CollisionGroups::new(Group::GROUP_4, Group::GROUP_3),
                    ));
                }
            }

            player.slide_timer -= time.delta_seconds();

            if player.slide_timer <= 0.0 {
                player.is_sliding = false;
                player.slide_cube_active = false;
                player.slide_cube_offset = Vec2::ZERO; // Reset final
                if let Ok((cube_entity, _, _)) = cube_query.get_mut(player.slide_cube) {
                    commands.entity(cube_entity).remove::<Collider>();
                }

                // Activar movimiento de reducción
                if let Some(movement) = get_movement(movement_ids::SLIDE_CUBE_SHRINK) {
                    let duration_ticks = (movement.duration * TICK_RATE as f32) as u32;
                    player.active_movement = Some(PlayerMovement {
                        movement_id: movement_ids::SLIDE_CUBE_SHRINK,
                        start_tick: tick.0,
                        end_tick: tick.0 + duration_ticks,
                    });
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
