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

    let (network_tx, network_rx) = mpsc::channel(10000);
    let (input_tx, input_rx) = mpsc::channel::<shared::protocol::ClientMessage>(10000);

    let server_addr = args.server.clone();
    let player_name = args.name.clone();

    // Hilo de red con Runtime dedicado
    std::thread::spawn(move || {
        println!("🌐 [Red] Hilo iniciado");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Fallo al crear Runtime de Tokio");

        rt.block_on(async {
            start_network_client(server_addr, player_name, network_tx, input_rx).await;
        });
        println!("🌐 [Red] El hilo de red HA TERMINADO (start_network_client retornó)");
    });

    // Bevy
    println!("🎨 [Bevy] Intentando abrir ventana...");
    App::new()
        .insert_resource(bevy::winit::WinitSettings::game())
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("Haxball - {}", args.name),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(GameConfig::default())
        .insert_resource(NetworkReceiver(Arc::new(Mutex::new(network_rx))))
        .insert_resource(InputSender(input_tx))
        .insert_resource(MyPlayerId(None))
        .insert_resource(PreviousInput::default())
        .insert_resource(HeartbeatTimer(Timer::from_seconds(1.0, TimerMode::Repeating)))
        .add_systems(Startup, setup)
        // Lógica de red y entrada (frecuencia fija)
        .add_systems(FixedUpdate, (
            handle_input,              // Enviamos inputs al ritmo del tickrate
            process_network_messages,  // Procesamos paquetes llegados
        ))
        // Lógica visual y renderizado (frecuencia del monitor)
        .add_systems(Update, (
            sync_players,          // Spawnea o actualiza entidades
            interpolate_entities,  // Suaviza el movimiento entre posiciones de red
            camera_follow_player,  // La cámara debe seguir al jugador cada frame
        ))
        .run();

    println!("✅ [Bevy] App::run() ha finalizado normalmente");
}

fn debug_circle(mut gizmos: Gizmos) {
    gizmos.circle_2d(Vec2::ZERO, 100.0, Color::WHITE);
}

// ============================================================================
// RECURSOS
// ============================================================================

#[derive(Resource)]
struct NetworkReceiver(Arc<Mutex<mpsc::Receiver<ServerMessage>>>);

#[derive(Resource)]
struct InputSender(mpsc::Sender<shared::protocol::ClientMessage>);

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

async fn start_network_client(
    addr: String,
    player_name_arg: String, // Cambiamos el nombre para evitar colisiones
    network_tx: mpsc::Sender<ServerMessage>,
    mut input_rx: mpsc::Receiver<shared::protocol::ClientMessage>
) {
    println!("🔌 Conectando al servidor en {}...", addr);
    let socket = TcpStream::connect(addr).await.expect("Fallo al conectar");
    let (mut read_half, mut write_half) = socket.into_split();

    // CLONAMOS las variables necesarias antes de moverlas al hilo (spawn)
    let name_to_send = player_name_arg.clone();

    // --- TASK DE ENVÍO ---
    tokio::spawn(async move {
        // 1. Enviar JOIN
        let join_msg = ClientMessage::Join {
            player_name: name_to_send,
            input_type: NetworkInputType::Keyboard,
        };

        if let Ok(data) = bincode::serialize(&join_msg) {
            let _ = write_half.write_all(&(data.len() as u32).to_le_bytes()).await;
            let _ = write_half.write_all(&data).await;
        }

        // 2. ESPERAR SEÑAL DE BEVY:
        // El primer input que llega por 'input_rx' indica que Bevy ya cargó.
        if let Some(primer_input) = input_rx.recv().await {
            println!("🎮 Bevy e Intel Iris listos. Enviando READY...");

            let ready_msg = ClientMessage::Ready;
            if let Ok(data) = bincode::serialize(&ready_msg) {
                let _ = write_half.write_all(&(data.len() as u32).to_le_bytes()).await;
                let _ = write_half.write_all(&data).await;
            }

            // Enviamos ese primer input
            enviar_input_packet(&mut write_half, primer_input).await;
        }

        // 3. Loop normal de inputs (usando input_rx)
        while let Some(input) = input_rx.recv().await {
            enviar_input_packet(&mut write_half, input).await;
        }
    });

    // --- LOOP DE RECEPCIÓN (EL QUE YA TENÍAS) ---
    let mut buffer = vec![0u8; 65536];
    loop {
        let mut len_buf = [0u8; 4];
        // Timeout de 20s para dar tiempo a la GPU
        match tokio::time::timeout(std::time::Duration::from_secs(20), read_half.read_exact(&mut len_buf)).await {
            Ok(Ok(_)) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                if len > buffer.len() { buffer.resize(len, 0); }
                if let Err(_) = read_half.read_exact(&mut buffer[..len]).await { break; }

                if let Ok(msg) = bincode::deserialize::<ServerMessage>(&buffer[..len]) {
                    // Usamos try_send para no bloquear la red si Bevy está lento
                    if let Err(mpsc::error::TrySendError::Closed(_)) = network_tx.try_send(msg) {
                        break;
                    }
                }
            }
            _ => {
                println!("🔌 Timeout o error de conexión (20s sin datos)");
                break;
            }
        }
    }
}

// Antes: ...input: PlayerInput
async fn enviar_input_packet(
    write: &mut tokio::net::tcp::OwnedWriteHalf,
    msg: shared::protocol::ClientMessage // <--- CAMBIA ESTO
) {
    use tokio::io::AsyncWriteExt;

    // Serializamos el mensaje completo
    let serialized = bincode::serialize(&msg).expect("Fallo al serializar mensaje");

    // Opcional: Enviar el tamaño primero si tu servidor lo requiere,
    // pero si el servidor usa bincode::deserialize_from, esto suele bastar:
    if let Err(e) = write.write_all(&serialized).await {
        eprintln!("❌ Error enviando paquete al servidor: {}", e);
    }
}

// ============================================================================
// GAME SYSTEMS
// ============================================================================

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    input_sender: Res<InputSender>,) {
    // Cámara
    commands.spawn((Camera2dBundle::default(), MainCamera));

    // El Campo de Juego (Césped)
    commands.spawn(SpriteBundle {
        sprite: Sprite {
            color: Color::srgb(0.2, 0.4, 0.2), // Bevy 0.14 usa srgb
            custom_size: Some(Vec2::new(800.0, 500.0)), // Valores fijos temporales
            ..default()
        },
        transform: Transform::from_xyz(0.0, 0.0, -10.0),
        ..default()
    });

    if let Err(e) = input_sender.0.try_send(shared::protocol::ClientMessage::Ready) {
            println!("⚠️ Error al enviar Ready desde Bevy: {:?}", e);
        } else {
            println!("🎮 Bevy e Intel Iris listos. Enviando READY al servidor...");
        }

    println!("✅ Cliente configurado y campo listo");
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
        let msg = shared::protocol::ClientMessage::Input {
            sequence: 0, // Por ahora usamos 0
            input: input, // Aquí pasamos el PlayerInput que el sistema detectó
        };

        let _ = input_sender.0.try_send(msg);
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
        let mensaje = shared::protocol::ClientMessage::Input {
            sequence: 0, // Puedes usar 0 por ahora o un contador si tienes uno
            input: previous_input.0.clone(),
        };

        let _ = input_sender.0.try_send(mensaje);
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
                            interp.target_position = ps.position;
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
                                target_position: ps.position,
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

fn sync_players(
    mut commands: Commands,
    network_rx: Res<NetworkReceiver>,
    // Usamos el nombre completo para evitar ambigüedades
    mut query: Query<(Entity, &mut Transform, &shared::protocol::PlayerState)>,
) {
    let Ok(mut receiver) = network_rx.0.lock() else { return; };

    while let Ok(msg) = receiver.try_recv() {
        if let ServerMessage::GameState { players, .. } = msg {
            for network_player in players {
                let existing = query.iter_mut().find(|(_, _, p)| p.id == network_player.id);

                // Convertimos la posición de tupla (f32, f32) a Vec3 de Bevy
                let pos_vec3 = Vec3::new(network_player.position.x, network_player.position.y, 0.0);

                if let Some((_, mut transform, _)) = existing {
                    transform.translation = pos_vec3;
                } else {
                    println!("🆕 Spawneando jugador: {}", network_player.name);

                    commands.spawn((
                        SpriteBundle {
                            sprite: Sprite {
                                color: if network_player.id == 1 {
                                    Color::srgb(1.0, 0.0, 0.0)
                                } else {
                                    Color::srgb(0.0, 0.0, 1.0)
                                },
                                custom_size: Some(Vec2::splat(30.0)),
                                ..default()
                            },
                            transform: Transform::from_translation(pos_vec3),
                            ..default()
                        },
                        network_player.clone(),
                    ));
                }
            }
        }
    }
}
