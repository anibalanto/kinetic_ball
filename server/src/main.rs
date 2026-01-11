use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use shared::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use std::sync::{Arc, Mutex};

mod input;
use input::{GameAction, InputSource, NetworkInputSource};

fn main() {
    println!("🎮 Haxball Server - Iniciando...");

    let (network_tx, network_rx) = mpsc::channel(100);
    let network_state = Arc::new(Mutex::new(NetworkState { next_player_id: 1 }));

    std::thread::spawn(move || {
        start_network_server(network_tx, network_state);
    });

    App::new()
        .add_plugins(
            MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            )),
        )
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .insert_resource(RapierConfiguration {
            gravity: Vec2::ZERO,
            physics_pipeline_active: true,
            query_pipeline_active: true,
            timestep_mode: TimestepMode::Fixed {
                dt: 1.0 / 60.0,
                substeps: 2,
            },
            force_update_from_transform_changes: false,
            scaled_shape_subdivision: 10,
        })
        .insert_resource(GameConfig::default())
        .insert_resource(NetworkReceiver(network_rx))
        .insert_resource(GameTick(0))
        .insert_resource(BroadcastTimer(Timer::from_seconds(
            1.0 / 60.0,
            TimerMode::Repeating,
        ))) // 60 Hz
        .init_resource::<GameInputManager>()
        .add_systems(Startup, setup_game)
        .add_systems(
            FixedUpdate,
            (
                process_network_messages,
                move_players,
                look_at_ball,
                charge_kick,
                charge_curve,
                kick_ball,
                apply_magnus_effect,
                attract_ball,
                auto_touch_ball,
                update_ball_damping,
                broadcast_game_state,
            ),
        )
        .run();
}

// ============================================================================
// RECURSOS Y COMPONENTES
// ============================================================================

#[derive(Resource)]
struct NetworkReceiver(mpsc::Receiver<NetworkEvent>);

#[derive(Resource)]
struct GameTick(u32);

#[derive(Resource)]
struct BroadcastTimer(Timer);

/// GameInputManager - Igual interfaz que RustBall pero usando NetworkInputSource
#[derive(Resource)]
pub struct GameInputManager {
    sources: std::collections::HashMap<u32, NetworkInputSource>,
}

impl GameInputManager {
    pub fn new() -> Self {
        Self {
            sources: std::collections::HashMap::new(),
        }
    }

    pub fn add_player(&mut self, player_id: u32) {
        self.sources.insert(player_id, NetworkInputSource::new());
    }

    pub fn update_input(&mut self, player_id: u32, input: PlayerInput) {
        if let Some(source) = self.sources.get_mut(&player_id) {
            source.set_input(input);
        }
    }

    pub fn is_pressed(&self, player_id: u32, action: GameAction) -> bool {
        self.sources
            .get(&player_id)
            .map(|s| s.is_pressed(action))
            .unwrap_or(false)
    }

    pub fn just_pressed(&self, player_id: u32, action: GameAction) -> bool {
        self.sources
            .get(&player_id)
            .map(|s| s.just_pressed(action))
            .unwrap_or(false)
    }

    pub fn just_released(&self, player_id: u32, action: GameAction) -> bool {
        self.sources
            .get(&player_id)
            .map(|s| s.just_released(action))
            .unwrap_or(false)
    }
}

impl Default for GameInputManager {
    fn default() -> Self {
        Self::new()
    }
}

// Estructura igual a RustBall - Player referencia a Sphere
#[derive(Component)]
struct Player {
    sphere: Entity, // Referencia a la entidad física (igual que RustBall)
    id: u32,        // Reemplaza input_type de RustBall
    name: String,
    kick_charge: f32,
    kick_charging: bool,
    curve_charge: f32,
    curve_charging: bool,
    tx: mpsc::Sender<ServerMessage>,
    pub is_ready: bool,
}

// Marker component para la entidad física del jugador (igual que RustBall)
#[derive(Component)]
struct Sphere;

#[derive(Component)]
struct Ball {
    angular_velocity: f32,
}

// ============================================================================
// NETWORK STATE
// ============================================================================

struct NetworkState {
    next_player_id: u32,
}

enum NetworkEvent {
    NewPlayer {
        id: u32,
        name: String,
        tx: mpsc::Sender<ServerMessage>,
    },
    PlayerInput {
        id: u32,
        input: PlayerInput,
    },
    PlayerDisconnected {
        id: u32,
    },
    PlayerReady {
        id: u32,
    },
}

// ============================================================================
// NETWORK SERVER
// ============================================================================

fn start_network_server(event_tx: mpsc::Sender<NetworkEvent>, state: Arc<Mutex<NetworkState>>) {
    // Creamos un runtime de Tokio dedicado para la red
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("No se pudo crear el runtime de Tokio");

    rt.block_on(async {
        let listener = TcpListener::bind("0.0.0.0:9000")
            .await
            .expect("No se pudo enlazar el servidor al puerto 9000");

        println!("🌐 Servidor escuchando en 0.0.0.0:9000");

        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    println!("📥 Nueva conexión desde: {}", addr);

                    // Configuración del socket para reducir latencia (Importante para juegos)
                    let _ = socket.set_nodelay(true);

                    let event_tx = event_tx.clone();
                    let state = state.clone();

                    tokio::spawn(handle_client(socket, event_tx, state));
                }
                Err(e) => {
                    eprintln!("❌ Error aceptando conexión: {}", e);
                }
            }
        }
    });
}

async fn handle_client(
    socket: TcpStream,
    event_tx: mpsc::Sender<NetworkEvent>,
    state: Arc<Mutex<NetworkState>>,
) {
    let addr = socket.peer_addr().unwrap();
    println!(
        "[{:?}] 🔗 Iniciando handler para {}",
        std::time::Instant::now(),
        addr
    );

    let mut buffer = vec![0u8; 4096];
    let mut player_id: Option<u32> = None;

    let (tx, mut rx) = mpsc::channel::<ServerMessage>(1000); // Buffer más grande

    let (mut read_half, mut write_half) = socket.into_split();

    // Task de envío optimizado
    tokio::spawn(async move {
        let mut msg_count = 0;
        while let Some(msg) = rx.recv().await {
            if let Ok(data) = bincode::serialize(&msg) {
                msg_count += 1;
                let len = data.len() as u32;

                // Unificamos en un solo buffer
                let mut packet = Vec::with_capacity(4 + data.len());
                packet.extend_from_slice(&len.to_le_bytes());
                packet.extend_from_slice(&data);

                if let Err(e) = write_half.write_all(&packet).await {
                    println!("❌ Error en socket para {}: {:?}", addr, e);
                    break;
                }

                // Log cada 100 mensajes para no saturar la consola
                if msg_count % 100 == 0 {
                    println!(
                        "[{:?}] 📤 {} msgs enviados a {}",
                        std::time::Instant::now(),
                        msg_count,
                        addr
                    );
                }
            }
        }
    });

    println!(
        "[{:?}] 📥 Iniciando loop de recepción para {}",
        std::time::Instant::now(),
        addr
    );

    loop {
        let mut len_buf = [0u8; 4];

        match read_half.read_exact(&mut len_buf).await {
            Ok(_) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                println!("[SERVER] Expecting {} bytes from client.", len);
                println!(
                    "[{:?}] 📩 Recibiendo mensaje de {} bytes desde {}",
                    std::time::Instant::now(),
                    len,
                    addr
                );

                if len > buffer.len() {
                    buffer.resize(len, 0);
                }

                if let Err(e) = read_half.read_exact(&mut buffer[..len]).await {
                    println!(
                        "[{:?}] ❌ Error leyendo datos desde {}: {:?}",
                        std::time::Instant::now(),
                        addr,
                        e
                    );
                    break;
                }
                println!(
                    "[{:?}] ✅ Mensaje completo recibido desde {}",
                    std::time::Instant::now(),
                    addr
                );

                println!("DEBUG: Bytes recibidos: {:02X?}", &buffer[..len]);
                match bincode::deserialize::<ClientMessage>(&buffer[..len]) {
                    Ok(msg) => {
                        println!(
                            "[{:?}] 🔍 Mensaje deserializado: {:?}",
                            std::time::Instant::now(),
                            std::mem::discriminant(&msg)
                        );
                        match msg {
                            ClientMessage::Ready => {
                                if let Some(id) = player_id {
                                    println!("✅ Jugador {} marcado como READY", id);
                                    let _ = event_tx.send(NetworkEvent::PlayerReady { id }).await;
                                } else {
                                    println!("⚠️ Recibido Ready sin player_id asignado");
                                }
                            }
                            ClientMessage::Join { player_name, .. } => {
                                let id = {
                                    let mut s = state.lock().unwrap();
                                    let id = s.next_player_id;
                                    s.next_player_id += 1;
                                    id
                                };

                                player_id = Some(id);

                                let _ = tx
                                    .send(ServerMessage::Welcome {
                                        player_id: id,
                                        game_config: GameConfig::default(),
                                    })
                                    .await;

                                let _ = event_tx
                                    .send(NetworkEvent::NewPlayer {
                                        id,
                                        name: player_name.clone(),
                                        tx: tx.clone(),
                                    })
                                    .await;

                                println!("🎮 Jugador {} conectado: {}", id, player_name);
                            }

                            ClientMessage::Input { input, .. } => {
                                // Usamos {:?} para el Option
                                println!("📥 [Server] Recibido Input de jugador {:?}: Up={}, Down={}, Left={}, Right={}",
                                    player_id, input.move_up, input.move_down, input.move_left, input.move_right);

                                if let Some(id) = player_id {
                                    let _ = event_tx
                                        .send(NetworkEvent::PlayerInput { id, input })
                                        .await;
                                }
                            }

                            ClientMessage::Ping { timestamp } => {
                                println!(
                                    "[{:?}] 🏓 Ping recibido de jugador {:?}",
                                    std::time::Instant::now(),
                                    player_id
                                );
                                let _ = tx
                                    .send(ServerMessage::Pong {
                                        client_timestamp: timestamp,
                                        server_timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_millis()
                                            as u64,
                                    })
                                    .await;
                            }
                        }
                    }
                    Err(_) => {
                        println!(
                            "[{:?}] ⚠️  Error deserializando mensaje desde {}",
                            std::time::Instant::now(),
                            addr
                        );
                    }
                }
            }
            Err(e) => {
                println!(
                    "[{:?}] ❌ Error leyendo desde {}: {:?}",
                    std::time::Instant::now(),
                    addr,
                    e
                );
                break;
            }
        }
    }
}

// ============================================================================
// GAME SETUP
// ============================================================================

fn setup_game(mut commands: Commands, config: Res<GameConfig>) {
    println!("⚽ Configurando juego...");

    // Crear pelota
    commands.spawn((
        Ball {
            angular_velocity: 0.0,
        },
        TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, 0.0)),
        RigidBody::Dynamic,
        Collider::ball(config.ball_radius),
        Velocity::zero(),
        AdditionalMassProperties::Mass(config.ball_mass),
        Friction {
            coefficient: config.ball_friction,
            combine_rule: CoefficientCombineRule::Average,
        },
        Restitution {
            coefficient: config.ball_restitution,
            combine_rule: CoefficientCombineRule::Average,
        },
        Damping {
            linear_damping: config.ball_linear_damping,
            angular_damping: config.ball_angular_damping,
        },
        ExternalImpulse::default(),
        ExternalForce::default(),
    ));

    // Crear paredes
    let wall_thickness = 10.0;
    let wall_collision = CollisionGroups::new(Group::GROUP_1, Group::ALL);

    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(config.arena_width / 2.0, wall_thickness),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(0.0, config.arena_height / 2.0, 0.0),
        GlobalTransform::default(),
    ));

    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(config.arena_width / 2.0, wall_thickness),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(0.0, -config.arena_height / 2.0, 0.0),
        GlobalTransform::default(),
    ));

    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(wall_thickness, config.arena_height / 2.0),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(-config.arena_width / 2.0, 0.0, 0.0),
        GlobalTransform::default(),
    ));

    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(wall_thickness, config.arena_height / 2.0),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(config.arena_width / 2.0, 0.0, 0.0),
        GlobalTransform::default(),
    ));

    println!("✅ Juego configurado");
}

fn process_network_messages(
    mut commands: Commands,
    mut network_rx: ResMut<NetworkReceiver>,
    config: Res<GameConfig>,
    mut game_input: ResMut<GameInputManager>,
    mut players: Query<(&mut Player, Entity)>,
) {
    while let Ok(event) = network_rx.0.try_recv() {
        match event {
            NetworkEvent::NewPlayer { id, name, tx } => {
                // Agregar jugador al GameInputManager
                game_input.add_player(id);

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
                        CollisionGroups::new(Group::GROUP_4, Group::ALL),
                        Friction {
                            coefficient: config.sphere_friction,
                            combine_rule: CoefficientCombineRule::Average,
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

                // Spawn lógica del jugador (Player) - igual que RustBall
                commands.spawn(Player {
                    sphere: sphere_entity,
                    id,
                    name: name.clone(),
                    kick_charge: 0.0,
                    kick_charging: false,
                    curve_charge: 0.0,
                    curve_charging: false,
                    tx,
                    is_ready: false,
                });

                println!("✅ Jugador {} spawneado: {}", id, name);
            }

            NetworkEvent::PlayerInput { id, input } => {
                // Actualizar GameInputManager (igual que RustBall actualiza sus fuentes de input)
                game_input.update_input(id, input);
            }

            NetworkEvent::PlayerDisconnected { id } => {
                for (player, entity) in players.iter() {
                    if player.id == id {
                        // Despawnear tanto Player como Sphere (igual que RustBall)
                        commands.entity(player.sphere).despawn();
                        commands.entity(entity).despawn();
                        println!("❌ Jugador {} removido", id);
                        break;
                    }
                }
            }

            NetworkEvent::PlayerReady { id } => {
                for (mut player, _) in players.iter_mut() {
                    if player.id == id {
                        player.is_ready = true;
                        println!("✅ Jugador {} marcado como READY en el loop de juego", id);
                        break;
                    }
                }
            }
        }
    }
}
// ============================================================================
// SISTEMAS DE FÍSICA - Agregar al final de server/src/main.rs
// ============================================================================

// Sistema copiado EXACTAMENTE de RustBall - move_player
fn move_players(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    players: Query<&Player>,
    mut sphere_query: Query<&mut Velocity, With<Sphere>>,
) {
    for player in players.iter() {
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
                // Reducir velocidad según el modo (igual que RustBall)
                let speed_multiplier = if !game_input.is_pressed(player_id, GameAction::Sprint) {
                    0.75 // 65% de velocidad cuando no corre
                } else {
                    1.0
                };
                velocity.linvel =
                    movement.normalize_or_zero() * config.player_speed * speed_multiplier;
            } else {
                velocity.linvel = Vec2::ZERO;
            }
        }
    }
}

// Sistema copiado de RustBall - charge_kick
fn charge_kick(
    game_input: Res<GameInputManager>,
    mut players: Query<&mut Player>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        let player_id = player.id;
        let kick_pressed = game_input.is_pressed(player_id, GameAction::Kick);
        let kick_just_pressed = game_input.just_pressed(player_id, GameAction::Kick);

        let should_start = kick_just_pressed && !player.kick_charging;

        if should_start {
            player.kick_charging = true;
            player.kick_charge = 0.0;
        }

        if kick_pressed && player.kick_charging {
            player.kick_charge += 2.0 * time.delta_seconds();
            if player.kick_charge > 1.0 {
                player.kick_charge = 1.0;
            }
        }

        let should_release = !kick_pressed && player.kick_charging;
        if should_release {
            player.kick_charging = false;
        }
    }
}

// Sistema copiado de RustBall - charge_curve
fn charge_curve(
    game_input: Res<GameInputManager>,
    mut players: Query<&mut Player>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        let player_id = player.id;
        let mut curve_factor: f32 = 0.0;

        let curve_left = game_input.is_pressed(player_id, GameAction::CurveLeft);
        let curve_right = game_input.is_pressed(player_id, GameAction::CurveRight);

        if curve_left != curve_right {
            if curve_right {
                curve_factor += 1.0;
            } else if curve_left {
                curve_factor -= 1.0;
            }
        }

        if curve_factor != 0.0 {
            player.curve_charge += curve_factor * 3.0 * time.delta_seconds();
            if player.curve_charge > 1.0 {
                player.curve_charge = 1.0;
            } else if player.curve_charge < -1.0 {
                player.curve_charge = -1.0;
            }
        } else if player.curve_charge.abs() > 0.001 {
            player.curve_charge *= 0.75;
        } else if player.curve_charge.abs() > 0.0 {
            player.curve_charge = 0.0;
        }
    }
}

fn look_at_ball(
    game_input: Res<GameInputManager>,
    player_query: Query<&Player>,
    mut sphere_query: Query<&mut Transform, With<Sphere>>,
    ball_query: Query<&Transform, (With<Ball>, Without<Sphere>)>,
) {
    if let Ok(ball_transform) = ball_query.get_single() {
        for player in player_query.iter() {
            if let Ok(mut sphere_transform) = sphere_query.get_mut(player.sphere) {
                let direction =
                    (ball_transform.translation - sphere_transform.translation).truncate();

                if direction.length() > 0.0 {
                    let mut angle = direction.y.atan2(direction.x);
                    let tilt_rad = 30.0f32.to_radians();

                    if game_input.is_pressed(player.id, GameAction::CurveRight) {
                        angle += tilt_rad;
                    } else if game_input.is_pressed(player.id, GameAction::CurveLeft) {
                        angle -= tilt_rad;
                    }

                    sphere_transform.rotation = Quat::from_rotation_z(angle);
                }
            }
        }
    }
}

// SISTEMA DE KICK MEJORADO - Usa impulso en vez de reemplazar velocidad
fn kick_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse, &mut Ball)>,
    sphere_query: Query<&Transform, With<Sphere>>,
    mut player_query: Query<&mut Player>,
) {
    for mut player in player_query.iter_mut() {
        let player_id = player.id;

        // Chequear si este jugador específico soltó el botón de pateo
        let should_kick = game_input.just_released(player_id, GameAction::Kick);

        if should_kick && player.kick_charge > 0.0 {
            // Chequear combas para este jugador
            let curve_right = game_input.is_pressed(player_id, GameAction::CurveRight)
                || game_input.just_released(player_id, GameAction::CurveRight);
            let curve_left = game_input.is_pressed(player_id, GameAction::CurveLeft)
                || game_input.just_released(player_id, GameAction::CurveLeft);

            let auto_curve = if curve_right {
                -1.0
            } else if curve_left {
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

                        let final_curve = if auto_curve != 0.0 {
                            auto_curve
                        } else {
                            player.curve_charge
                        };

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
            player.kick_charge = 0.0;
        }
    }
}

fn apply_magnus_effect(
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
            // Resetear spin cuando la pelota está casi quieta
            ball.angular_velocity = 0.0;
        }
    }
}

// SISTEMA DE ATRACCIÓN MEJORADO - Usa fuerza gradual en vez de reemplazar velocidad
fn attract_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    player_query: Query<(&Player, &Transform), With<Sphere>>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse, &mut Velocity), With<Ball>>,
) {
    for (player, player_transform) in player_query.iter() {
        let player_id = player.id;

        // Con Sprint no hay interacción con la pelota
        if game_input.is_pressed(player_id, GameAction::Sprint) {
            continue;
        }

        if !game_input.is_pressed(player_id, GameAction::StopInteract) {
            for (ball_transform, mut impulse, mut velocity) in ball_query.iter_mut() {
                let diff = player_transform.translation - ball_transform.translation;
                let distance = diff.truncate().length();

                // Radio de "pegado" - cuando está muy cerca, la pelota se queda pegada
                let stick_radius = config.sphere_radius + 25.0;

                if distance < stick_radius && distance > 1.0 {
                    // Efecto pegado: frenar la pelota y atraerla suavemente
                    let direction = diff.truncate().normalize_or_zero();

                    // Frenar la velocidad de la pelota (damping fuerte)
                    velocity.linvel *= 0.85;

                    // Atracción suave hacia el jugador
                    let stick_force = direction * 5000.0;
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

// Sistema de auto-toque: Da un pequeño impulso a la pelota cuando está muy cerca y corriendo
fn auto_touch_ball(
    game_input: Res<GameInputManager>,
    config: Res<GameConfig>,
    player_query: Query<(&Player, &Transform), With<Sphere>>,
    mut ball_query: Query<(&Transform, &mut ExternalImpulse), With<Ball>>,
) {
    // Radio de auto-toque (más pequeño que kick_distance_threshold)
    let auto_touch_radius = config.sphere_radius + config.ball_radius + 25.0;
    let auto_touch_force = 500.0; // Fuerza muy suave

    for (player, player_transform) in player_query.iter() {
        let player_id = player.id;

        // No auto-tocar si no está en sprint
        // o si está soltando la pelota intencionalmente
        if !game_input.is_pressed(player_id, GameAction::Sprint)
            || game_input.is_pressed(player_id, GameAction::StopInteract)
        {
            continue;
        }

        for (ball_transform, mut impulse) in ball_query.iter_mut() {
            let diff = ball_transform.translation - player_transform.translation;
            let distance = diff.truncate().length();

            // Solo aplicar auto-toque cuando está muy cerca
            if distance < auto_touch_radius && distance > 1.0 {
                // Dirección desde el jugador hacia la pelota
                let direction = diff.truncate().normalize_or_zero();

                // Aplicar un impulso muy suave
                let touch_impulse = direction * auto_touch_force;
                impulse.impulse += touch_impulse;
            }
        }
    }
}

fn update_ball_damping(
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

fn broadcast_game_state(
    time: Res<Time>,
    mut broadcast_timer: ResMut<BroadcastTimer>,
    mut tick: ResMut<GameTick>,
    players: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), With<Sphere>>,
    ball: Query<(&Transform, &Velocity, &Ball), Without<Sphere>>,
) {
    // Actualizar timer
    broadcast_timer.0.tick(time.delta());

    // Solo enviar cuando el timer se completa (30 veces por segundo)
    if !broadcast_timer.0.just_finished() {
        return;
    }

    tick.0 += 1;

    // Construir estado
    let player_states: Vec<PlayerState> = players
        .iter()
        .filter_map(|player| {
            if let Ok((transform, velocity)) = sphere_query.get(player.sphere) {
                // Extraer rotación Z del quaternion
                let (_, _, rotation_z) = transform.rotation.to_euler(EulerRot::XYZ);

                Some(PlayerState {
                    id: player.id,
                    name: player.name.clone(),
                    position: Vec2::new(transform.translation.x, transform.translation.y),
                    velocity: (velocity.linvel.x, velocity.linvel.y),
                    rotation: rotation_z,
                    kick_charge: player.kick_charge,
                    kick_charging: player.kick_charging,
                    curve_charge: player.curve_charge,
                    curve_charging: player.curve_charging,
                })
            } else {
                None
            }
        })
        .collect();

    let ball_state = if let Ok((transform, velocity, ball)) = ball.get_single() {
        BallState {
            position: (transform.translation.x, transform.translation.y),
            velocity: (velocity.linvel.x, velocity.linvel.y),
            angular_velocity: ball.angular_velocity,
        }
    } else {
        BallState {
            position: (0.0, 0.0),
            velocity: (0.0, 0.0),
            angular_velocity: 0.0,
        }
    };

    let game_state = ServerMessage::GameState {
        tick: tick.0,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        players: player_states,
        ball: ball_state,
    };

    // Enviar a todos los jugadores
    for player in players.iter() {
        if player.is_ready {
            // Usar try_send - si falla (buffer lleno), es OK, enviamos el siguiente frame
            match player.tx.try_send(game_state.clone()) {
                Ok(_) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Buffer lleno, saltear este frame (el cliente está atrasado)
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // El jugador se desconectó
                }
            }
        }
    }
}
