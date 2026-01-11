use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use shared::*;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use std::sync::{Arc, Mutex};

fn main() {
    println!("🎮 Haxball Server - Iniciando...");

    let (network_tx, network_rx) = mpsc::channel(100);
    let network_state = Arc::new(Mutex::new(NetworkState { next_player_id: 1 }));

    std::thread::spawn(move || {
        start_network_server(network_tx, network_state);
    });

    App::new()
        .add_plugins(MinimalPlugins)
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
        .insert_resource(BroadcastTimer(Timer::from_seconds(1.0 / 30.0, TimerMode::Repeating))) // 30 Hz
        .add_systems(Startup, setup_game)
        .add_systems(FixedUpdate, (
            process_network_messages,
            move_players,
            charge_kick,
            charge_curve,
            kick_ball,
            apply_magnus_effect,
            attract_ball,
            update_ball_damping,
            broadcast_game_state,
        ))
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

#[derive(Component)]
struct Player {
    id: u32,
    name: String,
    input: PlayerInput,
    kick_charge: f32,
    kick_charging: bool,
    curve_charge: f32,
    curve_charging: bool,
    tx: mpsc::Sender<ServerMessage>,
    pub is_ready: bool,
}

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
        tx: mpsc::Sender<ServerMessage>
    },
    PlayerInput {
        id: u32,
        input: PlayerInput
    },
    PlayerDisconnected {
        id: u32
    },
    PlayerReady {
        id: u32,
    },
}

// ============================================================================
// NETWORK SERVER
// ============================================================================

fn start_network_server(
    event_tx: mpsc::Sender<NetworkEvent>,
    state: Arc<Mutex<NetworkState>>,
) {
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
    println!("[{:?}] 🔗 Iniciando handler para {}", std::time::Instant::now(), addr);

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
                    println!("[{:?}] 📤 {} msgs enviados a {}", std::time::Instant::now(), msg_count, addr);
                }
            }
        }
    });

    println!("[{:?}] 📥 Iniciando loop de recepción para {}", std::time::Instant::now(), addr);

    loop {
        let mut len_buf = [0u8; 4];

        match read_half.read_exact(&mut len_buf).await {
            Ok(_) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                println!("[SERVER] Expecting {} bytes from client.", len);
                println!("[{:?}] 📩 Recibiendo mensaje de {} bytes desde {}", std::time::Instant::now(), len, addr);

                if len > buffer.len() {
                    buffer.resize(len, 0);
                }

                if let Err(e) = read_half.read_exact(&mut buffer[..len]).await {
                    println!("[{:?}] ❌ Error leyendo datos desde {}: {:?}", std::time::Instant::now(), addr, e);
                    break;
                }
                println!("[{:?}] ✅ Mensaje completo recibido desde {}", std::time::Instant::now(), addr);

                println!("DEBUG: Bytes recibidos: {:02X?}", &buffer[..len]);
                match bincode::deserialize::<ClientMessage>(&buffer[..len]) {
                    Ok(msg) => {
                        println!("[{:?}] 🔍 Mensaje deserializado: {:?}", std::time::Instant::now(), std::mem::discriminant(&msg));
                        match msg {
                            ClientMessage::Ready => {
                                // Enviamos un evento a Bevy o usamos un recurso compartido
                                // para marcar al jugador como listo en el mundo de Bevy.
                                let _ = event_tx.send(NetworkEvent::PlayerReady { id: player_id.unwrap() }).await;
                            }
                            ClientMessage::Join { player_name, .. } => {
                                let id = {
                                    let mut s = state.lock().unwrap();
                                    let id = s.next_player_id;
                                    s.next_player_id += 1;
                                    id
                                };

                                player_id = Some(id);

                                let _ = tx.send(ServerMessage::Welcome {
                                    player_id: id,
                                    game_config: GameConfig::default(),
                                }).await;

                                let _ = event_tx.send(NetworkEvent::NewPlayer {
                                    id,
                                    name: player_name.clone(),
                                    tx: tx.clone(),
                                }).await;

                                println!("🎮 Jugador {} conectado: {}", id, player_name);
                            }

                            ClientMessage::Input { input, .. } => {
                                if let Some(id) = player_id {
                                    let _ = event_tx.send(NetworkEvent::PlayerInput {
                                        id,
                                        input
                                    }).await;
                                }
                            }

                            ClientMessage::Ping { timestamp } => {
                                println!("[{:?}] 🏓 Ping recibido de jugador {:?}", std::time::Instant::now(), player_id);
                                let _ = tx.send(ServerMessage::Pong {
                                    client_timestamp: timestamp,
                                    server_timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis() as u64,
                                }).await;
                            }
                        }
                    }
                    Err(_) => {
                        println!("[{:?}] ⚠️  Error deserializando mensaje desde {}", std::time::Instant::now(), addr);
                    }
                }
            }
            Err(e) => {
                println!("[{:?}] ❌ Error leyendo desde {}: {:?}", std::time::Instant::now(), addr, e);
                break;
            }
        }
    }
}

// ============================================================================
// GAME SETUP
// ============================================================================

fn setup_game(
    mut commands: Commands,
    config: Res<GameConfig>,
) {
    println!("⚽ Configurando juego...");

    // Crear pelota
    commands.spawn((
        Ball { angular_velocity: 0.0 },
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
    mut players: Query<(&mut Player, Entity)>,
) {
    while let Ok(event) = network_rx.0.try_recv() {
        match event {
            NetworkEvent::NewPlayer { id, name, tx } => {
                // Spawn player
                let spawn_x = ((id % 3) as f32 - 1.0) * 200.0;
                let spawn_y = ((id / 3) as f32 - 1.0) * 200.0;

                commands.spawn((
                    Player {
                        id,
                        name: name.clone(),
                        input: PlayerInput::default(),
                        kick_charge: 0.0,
                        kick_charging: false,
                        curve_charge: 0.0,
                        curve_charging: false,
                        tx,
                        is_ready: false,
                    },
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
                ));

                println!("✅ Jugador {} spawneado: {}", id, name);
            }

            NetworkEvent::PlayerInput { id, input } => {
                for (mut player, _) in players.iter_mut() {
                    if player.id == id {
                        player.input = input;
                        break;
                    }
                }
            }

            NetworkEvent::PlayerDisconnected { id } => {
                for (player, entity) in players.iter() {
                    if player.id == id {
                        commands.entity(entity).despawn();
                        println!("❌ Jugador {} removido", id);
                        break;
                    }
                }
            }

            NetworkEvent::PlayerReady { .. } => {
                // No hacemos nada aquí porque lo maneja handle_ready_events
            }
        }
    }
}
// ============================================================================
// SISTEMAS DE FÍSICA - Agregar al final de server/src/main.rs
// ============================================================================

fn move_players(
    config: Res<GameConfig>,
    time: Res<Time>,
    mut players: Query<(&Player, &mut Velocity)>,
) {
    for (player, mut velocity) in players.iter_mut() {
        let input = &player.input;
        let mut movement = Vec2::ZERO;

        if input.move_up { movement.y += 1.0; }
        if input.move_down { movement.y -= 1.0; }
        if input.move_left { movement.x -= 1.0; }
        if input.move_right { movement.x += 1.0; }

        if movement.length() > 0.0 {
            let speed_multiplier = if !input.stop_interact {
                0.65 // 65% velocidad cuando controla pelota
            } else {
                1.0
            };
            velocity.linvel = movement.normalize() * config.player_speed * speed_multiplier;
        } else {
            velocity.linvel = Vec2::ZERO;
        }
    }
}

fn charge_kick(
    mut players: Query<&mut Player>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        let input = &player.input;

        if input.kick {
            if !player.kick_charging {
                player.kick_charging = true;
                player.kick_charge = 0.0;
            }
            player.kick_charge = (player.kick_charge + time.delta_seconds() * 2.0).min(1.0);
        } else if player.kick_charging {
            player.kick_charging = false;
        }
    }
}

fn charge_curve(
    mut players: Query<&mut Player>,
    time: Res<Time>,
) {
    for mut player in players.iter_mut() {
        let input = &player.input;

        if input.curve_left || input.curve_right {
            if !player.curve_charging {
                player.curve_charging = true;
                player.curve_charge = 0.0;
            }
            player.curve_charge = (player.curve_charge + time.delta_seconds() * 2.0).min(1.0);
        } else if player.curve_charging {
            player.curve_charging = false;
        }
    }
}

fn kick_ball(
    config: Res<GameConfig>,
    mut players: Query<(&mut Player, &Transform)>,
    mut ball_query: Query<(&mut Velocity, &Transform, &mut Ball), Without<Player>>,
) {
    if let Ok((mut ball_velocity, ball_transform, mut ball)) = ball_query.get_single_mut() {
        for (mut player, player_transform) in players.iter_mut() {
            let input = &player.input;

            // Detectar release del kick
            if !input.kick && player.kick_charge > 0.0 {
                let distance = (ball_transform.translation - player_transform.translation).length();

                if distance < config.kick_distance_threshold {
                    // Calcular dirección
                    let mut direction = (ball_transform.translation - player_transform.translation)
                        .truncate()
                        .normalize_or_zero();

                    if direction == Vec2::ZERO {
                        direction = Vec2::new(1.0, 0.0);
                    }

                    // Aplicar curva
                    let curve_amount = player.curve_charge;
                    if input.curve_right && curve_amount > 0.0 {
                        let angle = 30.0f32.to_radians() * curve_amount;
                        let cos = angle.cos();
                        let sin = angle.sin();
                        direction = Vec2::new(
                            direction.x * cos - direction.y * sin,
                            direction.x * sin + direction.y * cos,
                        );
                    } else if input.curve_left && curve_amount > 0.0 {
                        let angle = -30.0f32.to_radians() * curve_amount;
                        let cos = angle.cos();
                        let sin = angle.sin();
                        direction = Vec2::new(
                            direction.x * cos - direction.y * sin,
                            direction.x * sin + direction.y * cos,
                        );
                    }

                    // Aplicar impulso
                    let kick_strength = player.kick_charge.powf(0.7);
                    let impulse = direction * config.kick_force * kick_strength;
                    ball_velocity.linvel += impulse;

                    // Aplicar spin
                    if curve_amount > 0.0 {
                        let spin_direction = if input.curve_right { -1.0 } else { 1.0 };
                        let spin_force = config.spin_transfer * curve_amount * kick_strength * spin_direction;
                        ball.angular_velocity = spin_force;
                    }
                }

                player.kick_charge = 0.0;
                player.curve_charge = 0.0;
            }
        }
    }
}

fn apply_magnus_effect(
    config: Res<GameConfig>,
    mut ball_query: Query<(&mut ExternalForce, &Velocity, &Ball)>,
) {
    for (mut force, velocity, ball) in ball_query.iter_mut() {
        let speed = velocity.linvel.length();

        if speed > 10.0 && ball.angular_velocity.abs() > 0.1 {
            let velocity_dir = velocity.linvel.normalize_or_zero();
            let perpendicular = Vec2::new(-velocity_dir.y, velocity_dir.x);
            let magnus_force = perpendicular * ball.angular_velocity * config.magnus_coefficient;
            force.force = magnus_force;
        } else {
            force.force = Vec2::ZERO;
        }
    }
}

fn attract_ball(
    config: Res<GameConfig>,
    players: Query<(&Player, &Transform)>,
    mut ball_query: Query<(&mut ExternalImpulse, &Transform), With<Ball>>,
) {
    if let Ok((mut ball_impulse, ball_transform)) = ball_query.get_single_mut() {
        for (player, player_transform) in players.iter() {
            let input = &player.input;

            if !input.stop_interact {
                let distance = (ball_transform.translation - player_transform.translation).length();

                if distance > config.attract_min_distance && distance < config.attract_max_distance {
                    let direction = (player_transform.translation - ball_transform.translation)
                        .truncate()
                        .normalize_or_zero();

                    let strength = 1.0 - ((distance - config.attract_min_distance)
                        / (config.attract_max_distance - config.attract_min_distance));

                    ball_impulse.impulse += direction * config.attract_force * strength * 0.016;
                }
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
    players: Query<(&Player, &Transform, &Velocity)>,
    ball: Query<(&Transform, &Velocity, &Ball), Without<Player>>,
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
        .map(|(player, transform, velocity)| PlayerState {
            id: player.id,
            name: player.name.clone(),
            position: (transform.translation.x, transform.translation.y),
            velocity: (velocity.linvel.x, velocity.linvel.y),
            rotation: 0.0,
            kick_charge: player.kick_charge,
            kick_charging: player.kick_charging,
            curve_charge: player.curve_charge,
            curve_charging: player.curve_charging,
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
    for (player, _, _) in players.iter() {
        if player.is_ready {
            // Usar try_send - si falla (buffer lleno), es OK, enviamos el siguiente frame
            match player.tx.try_send(game_state.clone()) {
                Ok(_) => {},
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
