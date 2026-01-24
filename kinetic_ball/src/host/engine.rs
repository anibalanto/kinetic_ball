// ============================================================================
// SISTEMAS DE FÍSICA DEL JUEGO
// ============================================================================

use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use matchbox_socket::PeerId;
use crate::shared::movements::{get_movement, movement_ids};
use crate::shared::protocol::PlayerMovement;
use crate::shared::{GameConfig, TICK_RATE};

use super::input::GameAction;
use super::host::{Ball, GameInputManager, GameTick, Player, SlideCube, Sphere};
use crate::host::map::converter::MapConverter;
use crate::host::map::loader;
use rand::Rng;

const DEFAULT_MAP: &str = include_str!("../../assets/cancha_grande.hbs");

pub fn setup_map(mut commands: Commands) {
    let map = loader::load_map_from_str(DEFAULT_MAP, "default_map").unwrap();
    let converter = MapConverter::new();
    converter.spawn_map_geometry(&mut commands, &map);
    info!("Mapa por defecto spawneado en el host");
}

/// Aplica el kick a la pelota con la curva y spin correspondientes
/// Retorna la dirección final después de aplicar la curva
pub fn apply_kick(
    base_direction: Vec2,
    kick_charge: Vec2,
    config: &GameConfig,
    impulse: &mut ExternalImpulse,
    ball: &mut Ball,
) -> Vec2 {
    let direction = base_direction;

    // Aplicamos el impulso de salida
    impulse.impulse = direction * (kick_charge.x * config.kick_force);

    // Aplicamos el torque inicial (Spin)
    let spin_force = kick_charge.y * config.spin_transfer * 10.0;
    impulse.torque_impulse = spin_force;
    ball.angular_velocity = spin_force;

    direction
}

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
            Transform::from_xyz(spawn_x, spawn_y, 0.0),
            GlobalTransform::default(),
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
    let cube_offset = Vec2::new(config.sphere_radius * 0.7, 0.0);

    let slide_cube_entity = commands
        .spawn((
            SlideCube { owner_id: id },
            Transform::from_xyz(spawn_x + cube_offset.x, spawn_y + cube_offset.y, 0.0)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
                .with_scale(Vec3::splat(1.0)),
            GlobalTransform::default(),
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
        kick_charge: Vec2::ZERO,
        kick_charging: false,
        kick_memory_timer: 0.0,
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
        mode_active: false,
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
                let run_stamin_cost = time.delta_secs() * config.run_stamin_coeficient_cost;

                // En modo cubo siempre corre, en modo normal depende de Sprint
                let should_run =
                    player.mode_active || game_input.is_pressed(player_id, GameAction::Sprint);

                let move_coeficient = if should_run && player.stamin > run_stamin_cost {
                    player.stamin -= run_stamin_cost;
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
// kick_charge.x = potencia (0 a 1)
// kick_charge.y = dirección de curva (+1.0 derecha, -1.0 izquierda, 0.0 sin curva)
pub fn charge_kick(
    game_input: Res<GameInputManager>,
    mut players: Query<&mut Player>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        // No cargar kick en modo cubo
        if player.mode_active {
            continue;
        }

        let player_id = player.id;

        // Cualquiera de los 3 botones inicia la carga
        let kick_pressed = game_input.is_pressed(player_id, GameAction::Kick);
        let curve_left_pressed = game_input.is_pressed(player_id, GameAction::CurveLeft);
        let curve_right_pressed = game_input.is_pressed(player_id, GameAction::CurveRight);

        let any_kick_button = kick_pressed || curve_left_pressed || curve_right_pressed;
        let just_pressed_kick = game_input.just_pressed(player_id, GameAction::Kick);
        let just_pressed_left = game_input.just_pressed(player_id, GameAction::CurveLeft);
        let just_pressed_right = game_input.just_pressed(player_id, GameAction::CurveRight);
        let just_pressed = just_pressed_kick || just_pressed_left || just_pressed_right;

        if just_pressed {
            player.kick_charging = true;
            player.kick_charge = Vec2::ZERO;
        }

        if any_kick_button && player.kick_charging {
            player.kick_charge.x += 2.0 * time.delta_secs();
            if player.kick_charge.x > 1.0 {
                player.kick_charge.x = 1.0;
            }
            // Establecer dirección de curva
            if curve_right_pressed {
                player.kick_charge.y = -1.0;
            } else if curve_left_pressed {
                player.kick_charge.y = 1.0;
            } else {
                player.kick_charge.y = 0.0;
            }
        }
    }
}

// Sistema que prepara el kick: memoriza la carga cuando sueltas el botón
// El kick real se aplica en detect_contact_and_kick cuando hay contacto
// kick_charge.x = potencia, kick_charge.y = dirección curva (+1 derecha, -1 izquierda)
pub fn prepare_kick_ball(game_input: Res<GameInputManager>, mut player_query: Query<&mut Player>) {
    for mut player in player_query.iter_mut() {
        // No preparar kick en modo cubo
        if player.mode_active {
            continue;
        }

        let player_id = player.id;

        let any_kick_button = game_input.is_pressed(player_id, GameAction::Kick)
            || game_input.is_pressed(player_id, GameAction::CurveLeft)
            || game_input.is_pressed(player_id, GameAction::CurveRight);

        let should_release_kick = !any_kick_button && player.kick_charging;

        if should_release_kick {
            player.kick_charging = false;

            if player.kick_charge.x > 0.0 {
                // Memorizar la potencia por 1 segundo
                // El kick se aplicará cuando haya contacto con la pelota
                player.kick_memory_timer = 1.0;
            }
        }
    }
}

pub fn look_at_ball(
    player_query: Query<&Player>,
    mut sphere_query: Query<&mut Transform, With<Sphere>>,
    ball_query: Query<&Transform, (With<Ball>, Without<Sphere>)>,
) {
    if let Ok(ball_transform) = ball_query.single() {
        for player in player_query.iter() {
            // Durante slide, NO mirar la pelota - mantener rotación del deslizamiento
            if player.is_sliding {
                continue;
            }

            if let Ok(mut sphere_transform) = sphere_query.get_mut(player.sphere) {
                let direction =
                    (ball_transform.translation - sphere_transform.translation).truncate();

                if direction.length() > 0.0 {
                    let angle = direction.y.atan2(direction.x);

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
        // No funciona en modo cubo
        if player.mode_active {
            continue;
        }

        let player_id = player.id;

        // Con Sprint no hay interacción con la pelota
        if game_input.is_pressed(player_id, GameAction::Sprint) {
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

// Sistema de empuje al caminar: aplica impulso a la pelota cuando el jugador la toca mientras camina
pub fn push_ball_on_contact(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    player_query: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), (With<Sphere>, Without<Ball>)>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse), (With<Ball>, Without<Sphere>)>,
) {
    // Radio de contacto: cuando los colliders se tocan
    let contact_radius = config.sphere_radius + config.ball_radius + 5.0;
    let push_force = 8000.0; // Fuerza de empuje base

    for player in player_query.iter() {
        // No funciona en modo cubo
        if player.mode_active {
            continue;
        }

        if player.not_interacting {
            continue;
        }

        // Solo aplicar al caminar (sin Sprint) - al correr usa touch_ball_while_sprinting
        if game_input.is_pressed(player.id, GameAction::Sprint) {
            continue;
        }

        if let Ok((player_transform, player_velocity)) = sphere_query.get(player.sphere) {
            let player_speed = player_velocity.linvel.length();

            // Solo empujar si el jugador se está moviendo
            if player_speed < 10.0 {
                continue;
            }

            for (ball_transform, mut impulse) in ball_query.iter_mut() {
                let diff = ball_transform.translation - player_transform.translation;
                let distance = diff.truncate().length();

                // Solo aplicar cuando están en contacto
                if distance < contact_radius && distance > 1.0 {
                    // Dirección del movimiento del jugador
                    let push_direction = player_velocity.linvel.normalize_or_zero();

                    // Impulso proporcional a la velocidad del jugador
                    let push_impulse =
                        push_direction * push_force * (player_speed / 100.0).min(3.0);
                    impulse.impulse += push_impulse;
                }
            }
        }
    }
}

// Sistema que detecta contacto jugador-pelota y aplica el kick si hay carga memorizada
pub fn detect_contact_and_kick(
    config: Res<GameConfig>,
    mut player_query: Query<&mut Player>,
    sphere_query: Query<&Transform, (With<Sphere>, Without<Ball>)>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse, &mut Ball), With<Ball>>,
) {
    let contact_radius = config.sphere_radius + config.ball_radius + 5.0;

    for mut player in player_query.iter_mut() {
        // No aplicar kick en modo cubo
        if player.mode_active {
            continue;
        }

        // Solo aplicar si hay carga memorizada y no está cargando activamente
        if player.kick_charge.x <= 0.0 || player.kick_charging {
            continue;
        }

        if let Ok(player_transform) = sphere_query.get(player.sphere) {
            for (ball_transform, mut impulse, mut ball) in ball_query.iter_mut() {
                let diff = ball_transform.translation - player_transform.translation;
                let distance = diff.truncate().length();

                if distance < contact_radius && distance > 1.0 {
                    let kick_dir = diff.truncate().normalize_or_zero();

                    apply_kick(
                        kick_dir,
                        player.kick_charge,
                        &config,
                        &mut impulse,
                        &mut ball,
                    );

                    // Consumir la carga
                    player.kick_charge = Vec2::ZERO;
                    player.kick_memory_timer = 0.0;
                }
            }
        }
    }
}

// Sistema para decrementar el timer de potencia memorizada y cancelar con StopInteract
pub fn update_kick_memory_timer(
    game_input: Res<GameInputManager>,
    time: Res<Time>,
    mut player_query: Query<&mut Player>,
) {
    for mut player in player_query.iter_mut() {
        // Cancelar con StopInteract
        if game_input.is_pressed(player.id, GameAction::StopInteract) {
            player.kick_charge = Vec2::ZERO;
            player.kick_memory_timer = 0.0;
            continue;
        }

        // Decrementar timer
        if player.kick_memory_timer > 0.0 {
            player.kick_memory_timer -= time.delta_secs();
            if player.kick_memory_timer <= 0.0 {
                player.kick_charge = Vec2::ZERO;
                player.kick_memory_timer = 0.0;
            }
        }
    }
}

pub fn auto_touch_ball_while_running(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    player_query: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), (With<Sphere>, Without<Ball>)>,
    mut ball_query: Query<(&Transform, &mut Velocity), With<Ball>>,
) {
    let activation_radius = config.sphere_radius + config.ball_radius + 5.0;
    let default_kick_force = 700.0;

    for player in player_query.iter() {
        // No funciona en modo cubo
        if player.mode_active {
            continue;
        }

        // Solo si hay carga memorizada, el kick lo maneja detect_contact_and_kick
        if player.kick_charge.x > 0.0 && !player.kick_charging {
            continue;
        }

        if !game_input.is_pressed(player.id, GameAction::Sprint)
            || game_input.is_pressed(player.id, GameAction::StopInteract)
        {
            continue;
        }

        if let Ok((player_transform, player_velocity)) = sphere_query.get(player.sphere) {
            let normalized_velocity =
                player_velocity.linvel.length() / (config.player_speed * config.run_coeficient);
            println!("normalized_velocity {}", normalized_velocity);
            if normalized_velocity < 1.0 {
                continue;
            }

            println!("auto_touch?");

            for (ball_transform, mut ball_velocity) in ball_query.iter_mut() {
                let p_pos = player_transform.translation.truncate();
                let b_pos = ball_transform.translation.truncate();
                let diff = b_pos - p_pos;
                let current_dist = diff.length();

                if current_dist < activation_radius {
                    let kick_dir = diff.normalize_or_zero();
                    // Comportamiento por defecto: fuerza fija sin curva
                    ball_velocity.linvel = kick_dir * default_kick_force;
                }
            }
        }
    }
}

fn changing_mode(
    player: &mut Player,
    commands: &mut Commands,
    config: &Res<GameConfig>,
    sphere_entity: Entity,
    cube_entity: Entity,
) {
    println!(
        "🔄 Jugador {} modo: {}",
        player.id,
        if player.mode_active { "CUBO" } else { "ESFERA" }
    );

    // Cambiar física según el modo

    if player.mode_active {
        // Modo CUBO: esfera chica, cubo grande con física
        commands
            .entity(sphere_entity)
            .remove::<Collider>()
            .insert(Collider::ball(config.sphere_radius * 0.3));

        // Cubo grande con colisiones
        let cube_size = config.sphere_radius * 1.2;
        commands.entity(cube_entity).insert((
            Collider::cuboid(cube_size, cube_size),
            CollisionGroups::new(Group::GROUP_4, Group::ALL ^ Group::GROUP_5),
            SolverGroups::new(Group::GROUP_4, Group::ALL ^ Group::GROUP_5),
            Restitution {
                coefficient: 0.8,
                combine_rule: CoefficientCombineRule::Max,
            },
        ));

        // Actualizar offset para modo cubo (cubo al centro)
        player.slide_cube_offset = Vec2::ZERO;
        player.slide_cube_scale = 2.5;
        player.slide_cube_active = true;
    } else {
        // Modo ESFERA: restaurar tamaños normales
        commands
            .entity(sphere_entity)
            .remove::<Collider>()
            .insert(Collider::ball(config.sphere_radius));

        // Quitar física del cubo
        commands
            .entity(cube_entity)
            .remove::<Collider>()
            .remove::<CollisionGroups>()
            .remove::<SolverGroups>()
            .remove::<Restitution>();

        // Restaurar offset normal
        player.slide_cube_offset = Vec2::new(config.sphere_radius * 0.7, 0.0);
        player.slide_cube_scale = 1.0;
        player.slide_cube_active = false;
    }
}

// Sistema de toggle de modo: Tab activa/desactiva el modo cubo
pub fn toggle_mode(
    mut commands: Commands,
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut player_query: Query<&mut Player>,
    mut sphere_query: Query<Entity, With<Sphere>>,
    mut cube_query: Query<Entity, With<SlideCube>>,
) {
    for mut player in player_query.iter_mut() {
        if game_input.just_pressed(player.id, GameAction::Mode) {
            player.mode_active = !player.mode_active;
            if let Ok(sphere_entity) = sphere_query.get_mut(player.sphere) {
                if let Ok(cube_entity) = cube_query.get_mut(player.slide_cube) {
                    changing_mode(
                        &mut player,
                        &mut commands,
                        &config,
                        sphere_entity,
                        cube_entity,
                    );
                }
            }
        } else if player.mode_active && player.stamin < 0.09 {
            player.mode_active = false;
            if let Ok(sphere_entity) = sphere_query.get_mut(player.sphere) {
                if let Ok(cube_entity) = cube_query.get_mut(player.slide_cube) {
                    changing_mode(
                        &mut player,
                        &mut commands,
                        &config,
                        sphere_entity,
                        cube_entity,
                    );
                }
            }
        }
    }
}

// Sistema de barrida en modo cubo: Kick adelante, CurveLeft 45° izq, CurveRight 45° der
pub fn detect_slide(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    tick: Res<GameTick>,
    mut player_query: Query<&mut Player>,
    mut sphere_query: Query<(&mut Velocity, &Transform), With<Sphere>>,
) {
    for mut player in player_query.iter_mut() {
        // Solo funciona en modo cubo
        if !player.mode_active || player.is_sliding {
            continue;
        }

        // Detectar dirección de barrida
        let forward = game_input.just_pressed(player.id, GameAction::Kick);
        let left_45 = game_input.just_pressed(player.id, GameAction::CurveLeft);
        let right_45 = game_input.just_pressed(player.id, GameAction::CurveRight);

        if !forward && !left_45 && !right_45 {
            continue;
        }

        if config.slide_stamin_cost > player.stamin {
            continue;
        }

        if let Ok((mut velocity, transform)) = sphere_query.get_mut(player.sphere) {
            // Dirección base: hacia donde mira el jugador
            let (_, _, angle) = transform.rotation.to_euler(EulerRot::XYZ);
            let base_dir = Vec2::new(angle.cos(), angle.sin());

            // Calcular dirección final según la tecla
            let slide_dir = if forward {
                base_dir
            } else if left_45 {
                // Rotar 45° a la izquierda
                let angle_45 = std::f32::consts::FRAC_PI_2;
                Vec2::new(
                    base_dir.x * angle_45.cos() - base_dir.y * angle_45.sin(),
                    base_dir.x * angle_45.sin() + base_dir.y * angle_45.cos(),
                )
            } else {
                // Rotar 45° a la derecha
                let angle_45 = -std::f32::consts::FRAC_PI_2;
                Vec2::new(
                    base_dir.x * angle_45.cos() - base_dir.y * angle_45.sin(),
                    base_dir.x * angle_45.sin() + base_dir.y * angle_45.cos(),
                )
            };

            player.is_sliding = true;
            player.slide_timer = 0.3;
            player.slide_direction = slide_dir;

            velocity.linvel =
                slide_dir * velocity.linvel.length().max(100.0) * config.speed_slide_coefficient;

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

pub fn execute_slide(
    mut commands: Commands,
    config: Res<GameConfig>,
    time: Res<Time>,
    tick: Res<GameTick>,
    mut player_query: Query<&mut Player>,
    mut sphere_query: Query<
        (&mut Velocity, &Transform),
        (With<Sphere>, Without<SlideCube>, Without<Ball>),
    >,
    mut cube_query: Query<
        (Entity, &mut Transform, Option<&Collider>),
        (With<SlideCube>, Without<Sphere>, Without<Ball>),
    >,
    mut ball_query: Query<
        (&Transform, &mut ExternalImpulse),
        (With<Ball>, Without<Sphere>, Without<SlideCube>),
    >,
) {
    for mut player in player_query.iter_mut() {
        if !player.is_sliding {
            continue;
        }

        if let Ok((mut sphere_velocity, sphere_transform)) = sphere_query.get_mut(player.sphere) {
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
                let player_pos = sphere_transform.translation.truncate();
                cube_transform.translation = (player_pos + player.slide_cube_offset).extend(2.0);
                cube_transform.scale = Vec3::splat(player.slide_cube_scale);

                // --- HITBOX DE IMPULSO MANUAL ---
                // Solo aplicamos el impulso en la fase de "ida" o "mantenimiento" (progress < 0.7)
                if progress < 0.7 {
                    for (ball_transform, mut ball_impulse) in ball_query.iter_mut() {
                        let cube_pos = cube_transform.translation.truncate();
                        let ball_pos = ball_transform.translation.truncate();
                        let dist = cube_pos.distance(ball_pos);

                        // Umbral de detección: tamaño del cubo + radio de la pelota
                        let hit_threshold = (player.slide_cube_scale
                            * (config.sphere_radius / 1.5))
                            + config.ball_radius;

                        if dist < hit_threshold {
                            // Aplicamos un impulso masivo en la dirección de la barrida
                            // Multiplicamos por un valor alto para que se note el impacto
                            ball_impulse.impulse =
                                player.slide_direction * config.slide_punch_force;

                            // Opcional: añadir un poco de "levantamiento" o efecto

                            // Generamos el valor aleatorio
                            let mut rng = rand::thread_rng();
                            let random_torque = rng.gen_range(0.0..config.slide_max_torque);

                            ball_impulse.torque_impulse = player.slide_direction.x * random_torque;
                        }
                    }
                }
                // --------------------------------

                if maybe_collider.is_none() && progress > 0.1 {
                    let size = (config.sphere_radius / 1.5) * player.slide_cube_scale;
                    commands.entity(cube_entity).insert((
                        Collider::cuboid(size / 2.0, size / 2.0),
                        CollisionGroups::new(Group::GROUP_4, Group::GROUP_3),
                        Restitution {
                            coefficient: 1.5,
                            combine_rule: CoefficientCombineRule::Max,
                        },
                    ));
                }
            }

            player.slide_timer -= time.delta_secs();

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

                sphere_velocity.linvel /= config.speed_slide_coefficient;
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
) {
    let player_diameter = config.sphere_radius * 2.0;
    let target_distance = player_diameter * 1.5;
    let activation_radius = config.sphere_radius + config.ball_radius + 50.0;

    for mut player in player_query.iter_mut() {
        // Solo funciona en modo cubo
        if !player.mode_active {
            continue;
        }

        if game_input.is_pressed(player.id, GameAction::Sprint) {
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
                    player.stamin += time.delta_secs() * config.stamin_coeficient_restore;
                }
            }
        }
    }
}
