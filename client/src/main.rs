use bevy::prelude::*;
use clap::Parser;
use shared::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use bevy_rapier2d::prelude::Velocity;
use shared::protocol::BallState as Ball;
use bevy::prelude::*;
use shared::protocol::{PlayerState, BallState, ServerMessage, ClientMessage, PlayerInput, NetworkInputType, GameConfig};

#[derive(Parser, Debug)]
#[command(name = "Haxball Client")]
#[command(about = "Cliente del juego Haxball", long_about = None)]
struct Args {
    /// Dirección del servidor (ej: localhost:9000 o 192.168.0.79:9000)
    #[arg(short, long, default_value = "localhost:9000")]
    server: String,

    /// Nombre del jugador
    #[arg(short, long, default_value = "Player")]
    name: String,
}

fn main() {
    let args = Args::parse();

    println!("🎮 Haxball Client - Iniciando...");
    println!("📡 Conectando a: {}", args.server);

    std::panic::set_hook(Box::new(|info| {
            println!("🚨 PANIC en el cliente: {:?}", info);
        }));

    // Iniciar conexión de red
    let (network_tx, network_rx) = mpsc::channel(100);
    let (input_tx, input_rx) = mpsc::channel(100);

    let server_addr = args.server.clone();
    let player_name = args.name.clone();

    std::thread::spawn(move || {
        start_network_client(server_addr, player_name, network_tx, input_rx);
    });

    // Iniciar juego con Bevy
    App::new()
        .add_plugins(MinimalPlugins)
        /* .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("Haxball - {}", args.name),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))*/
        .insert_resource(ClearColor(Color::srgb(0.2, 0.5, 0.2)))
        .insert_resource(NetworkReceiver(Arc::new(Mutex::new(network_rx))))
        .insert_resource(InputSender(input_tx))
        .insert_resource(MyPlayerId(None))
        .insert_resource(PreviousInput::default())
        .insert_resource(HeartbeatTimer(Timer::from_seconds(1.0, TimerMode::Repeating)))
        .insert_resource(GameConfig::default())
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, (
            //handle_input,
            //send_heartbeat,
            process_network_messages,
            //camera_follow_player,
            //interpolate_entities,
        ))
        .run();
}

// ============================================================================
// RECURSOS
// ============================================================================

#[derive(Resource)]
struct NetworkReceiver(Arc<Mutex<mpsc::Receiver<ServerMessage>>>);

#[derive(Resource)]
struct InputSender(mpsc::Sender<PlayerInput>);

#[derive(Resource)]
struct MyPlayerId(Option<u32>);

// ============================================================================
// COMPONENTES
// ============================================================================

#[derive(Component)]
struct RemotePlayer {
    id: u32,
    name: String,
}

#[derive(Component)]
struct RemoteBall;

#[derive(Component)]
struct MainCamera;

#[derive(Component)]
struct Interpolated {
    target_position: Vec2,
    target_velocity: Vec2,
    smoothing: f32,
}
// ============================================================================
// NETWORK CLIENT (Tokio)
// ============================================================================

#[tokio::main]
async fn start_network_client(
    server_addr: String,
    player_name: String,
    network_tx: mpsc::Sender<ServerMessage>,
    mut input_rx: mpsc::Receiver<PlayerInput>,
) {
    println!("🔌 Conectando al servidor...");

    let socket = match TcpStream::connect(&server_addr).await {
        Ok(s) => {
            println!("✅ Conectado al servidor!");
            s
        }
        Err(e) => {
            eprintln!("❌ Error conectando: {}", e);
            return;
        }
    };

    let (mut read_half, mut write_half) = socket.into_split();

    // Task para enviar input al servidor
    tokio::spawn(async move {
        println!("[{:?}] 📤 Task de envío de input iniciado", std::time::Instant::now());

        // Enviar mensaje de join
        let join_msg = ClientMessage::Join {
            player_name,
            input_type: NetworkInputType::Keyboard,
        };
        println!("[{:?}] 📨 Enviando Join message", std::time::Instant::now());
        if let Ok(data) = bincode::serialize(&join_msg) {
            let len = data.len() as u32;
            let _ = write_half.write_all(&len.to_le_bytes()).await;
            let _ = write_half.write_all(&data).await;
            println!("[{:?}] ✅ Join message enviado", std::time::Instant::now());
        }

        let mut sequence = 0u32;
        let mut msg_count = 0;

        // Enviar inputs
        while let Some(input) = input_rx.recv().await {
            msg_count += 1;
            if msg_count % 60 == 0 {
                println!("[{:?}] 📨 {} mensajes de input enviados", std::time::Instant::now(), msg_count);
            }

            let msg = ClientMessage::Input {
                sequence,
                input
            };
            sequence += 1;

            if let Ok(data) = bincode::serialize(&msg) {
                let len = data.len() as u32;
                println!("[CLIENT] Sending {} bytes to server.", len);
                if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                    break;
                }
                if write_half.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    });

    // Recibir mensajes del servidor
    // LOOP recepción robusto en client/src/main.rs
    let mut buffer = vec![0u8; 65536]; // Buffer de 64KB
    loop {
        let mut len_buf = [0u8; 4];
        // Usamos un timeout corto para no quedar colgados si el socket muere silenciosamente
        match tokio::time::timeout(std::time::Duration::from_secs(20), read_half.read_exact(&mut len_buf)).await {
            Ok(Ok(_)) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                if len > buffer.len() { buffer.resize(len, 0); }

                if let Err(_) = read_half.read_exact(&mut buffer[..len]).await { break; }

                if let Ok(msg) = bincode::deserialize::<ServerMessage>(&buffer[..len]) {
                    // IMPORTANTE: Usamos try_send o un canal más grande para no bloquear la RED
                    // si Bevy está ocupado inicializando la GPU.
                    if let Err(e) = network_tx.try_send(msg) {
                        if let mpsc::error::TrySendError::Full(_) = e {
                            // Si el canal está lleno, simplemente ignoramos este GameState
                            // viejo para no saturar la memoria.
                        } else {
                            break; // Canal cerrado
                        }
                    }
                }
            }
            _ => {
                println!("🔌 Timeout o error de conexión");
                break;
            }
        }
    }
    println!("🔌 Desconectado del servidor");
}

// ============================================================================
// GAME SYSTEMS
// ============================================================================

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
) {
    // Cámara
    commands.spawn((
        Camera2dBundle {
            projection: OrthographicProjection {
                scale: 1.5,
                ..default()
            },
            ..default()
        },
        MainCamera,
    ));

    println!("✅ Cliente configurado");
}

// Resource para trackear el input anterior
#[derive(Resource, Default)]
struct PreviousInput(PlayerInput);

// Resource para el heartbeat timer
#[derive(Resource)]
struct HeartbeatTimer(Timer);

fn handle_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_sender: Res<InputSender>,
    my_player_id: Res<MyPlayerId>,
    mut previous_input: ResMut<PreviousInput>,
) {
    // Solo enviar input si ya tenemos un ID asignado
    if my_player_id.0.is_none() {
        return;
    }

    // Construir input del frame actual
    let input = PlayerInput {
        move_up: keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW),
        move_down: keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS),
        move_left: keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA),
        move_right: keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD),
        kick: keyboard.pressed(KeyCode::Space),
        curve_left: keyboard.pressed(KeyCode::KeyQ),
        curve_right: keyboard.pressed(KeyCode::KeyE),
        stop_interact: keyboard.pressed(KeyCode::ControlLeft),
        sprint: keyboard.pressed(KeyCode::ShiftLeft),
    };

    // Solo enviar si el input cambió (más eficiente)
    if input != previous_input.0 {
        let _ = input_sender.0.try_send(input);
        previous_input.0 = input;
    }
}

fn send_heartbeat(
    time: Res<Time>,
    mut heartbeat_timer: ResMut<HeartbeatTimer>,
    input_sender: Res<InputSender>,
    my_player_id: Res<MyPlayerId>,
    previous_input: Res<PreviousInput>,
) {
    // Solo enviar heartbeat si ya conectamos
    if my_player_id.0.is_none() {
        return;
    }

    heartbeat_timer.0.tick(time.delta());

    if heartbeat_timer.0.just_finished() {
        // Enviar el input actual como heartbeat (mantiene conexión viva)
        let _ = input_sender.0.try_send(previous_input.0);
    }
}

fn process_network_messages(
    mut commands: Commands,
    network_rx: Res<NetworkReceiver>,
    mut my_id: ResMut<MyPlayerId>,
    mut ball_q: Query<(&mut Interpolated, &RemoteBall), Without<RemotePlayer>>,
    mut players_q: Query<(&mut Interpolated, &RemotePlayer), Without<RemoteBall>>,
) {
    let mut rx = network_rx.0.lock().unwrap();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            ServerMessage::Welcome { player_id, .. } => {
                my_id.0 = Some(player_id);
            }
            ServerMessage::GameState { players, ball, .. } => {
                // Actualizar Pelota
                if let Ok((mut interp, _)) = ball_q.get_single_mut() {
                    interp.target_position = Vec2::new(ball.position.0, ball.position.1);
                    interp.target_velocity = Vec2::new(ball.velocity.0, ball.velocity.1);
                } else {
                    commands.spawn((
                        SpriteBundle {
                            sprite: Sprite { color: Color::WHITE, custom_size: Some(Vec2::splat(15.0)), ..default() },
                            ..default()
                        },
                        RemoteBall,
                        Interpolated {
                            target_position: Vec2::new(ball.position.0, ball.position.1),
                            target_velocity: Vec2::new(ball.velocity.0, ball.velocity.1),
                            smoothing: 20.0,
                        },
                    ));
                }

                // Actualizar Jugadores
                for ps in players {
                    let mut found = false;
                    for (mut interp, rp) in players_q.iter_mut() {
                        if rp.id == ps.id {
                            interp.target_position = Vec2::new(ps.position.0, ps.position.1);
                            interp.target_velocity = Vec2::new(ps.velocity.0, ps.velocity.1);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        commands.spawn((
                            SpriteBundle {
                                sprite: Sprite {
                                    color: if my_id.0 == Some(ps.id) { Color::srgb(0.2, 0.4, 1.0) } else { Color::srgb(1.0, 0.3, 0.3) },
                                    custom_size: Some(Vec2::splat(45.0)),
                                    ..default()
                                },
                                ..default()
                            },
                            RemotePlayer { id: ps.id, name: ps.name.clone() },
                            Interpolated {
                                target_position: Vec2::new(ps.position.0, ps.position.1),
                                target_velocity: Vec2::new(ps.velocity.0, ps.velocity.1),
                                smoothing: 15.0,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

// 3. Sistema de interpolación (Actualizado)
fn interpolate_entities(time: Res<Time>, mut q: Query<(&mut Transform, &Interpolated)>) {
    let dt = time.delta_seconds();
    for (mut transform, interp) in q.iter_mut() {
        let prediction_offset = interp.target_velocity * dt;
        let effective_target = interp.target_position + prediction_offset;
        let current_pos = transform.translation.truncate();
        let new_pos = current_pos.lerp(effective_target, dt * interp.smoothing);
        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

fn camera_follow_player(
    my_player_id: Res<MyPlayerId>,
    players: Query<(&RemotePlayer, &Transform), Without<MainCamera>>,
    mut camera: Query<&mut Transform, With<MainCamera>>,
) {
    if let Some(my_id) = my_player_id.0 {
        for (player, player_transform) in players.iter() {
            if player.id == my_id {
                if let Ok(mut cam_transform) = camera.get_single_mut() {
                    cam_transform.translation.x = player_transform.translation.x;
                    cam_transform.translation.y = player_transform.translation.y;
                }
                break;
            }
        }
    }
}
