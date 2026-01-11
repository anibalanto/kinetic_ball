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
    player_name_arg: String,
    network_tx: mpsc::Sender<ServerMessage>,
    mut input_rx: mpsc::Receiver<shared::protocol::ClientMessage>
) {
    println!("🔌 [Red] Intentando conectar al servidor en {}...", addr);
    let socket = TcpStream::connect(addr).await.expect("Fallo al conectar");
    let (mut read_half, mut write_half) = socket.into_split();
    println!("✅ [Red] Conectado exitosamente");

    // ==========================================
    // FASE 1: HANDSHAKE SINCRÓNICO
    // ==========================================

    // 1. Enviar JOIN
    let join_msg = ClientMessage::Join {
        player_name: player_name_arg.clone(),
        input_type: NetworkInputType::Keyboard,
    };
    if let Ok(data) = bincode::serialize(&join_msg) {
        println!("📤 [Red -> Servidor] Enviando JOIN...");
        write_half.write_all(&(data.len() as u32).to_le_bytes()).await.unwrap();
        write_half.write_all(&data).await.unwrap();
    }

    // 2. Leer WELCOME del servidor (bloqueante, pero está bien aquí)
    let mut len_buf = [0u8; 4];
    read_half.read_exact(&mut len_buf).await.expect("Error leyendo Welcome");
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut buffer = vec![0u8; len];
    read_half.read_exact(&mut buffer).await.expect("Error leyendo datos Welcome");

    match bincode::deserialize::<ServerMessage>(&buffer) {
        Ok(ServerMessage::Welcome { player_id, game_config }) => {
            println!("🎉 [Red] WELCOME recibido! Player ID: {}", player_id);
            // Enviar el Welcome a Bevy
            let _ = network_tx.send(ServerMessage::Welcome { player_id, game_config }).await;
        }
        _ => panic!("Se esperaba Welcome pero se recibió otro mensaje"),
    }

    // 3. Enviar READY inmediatamente después de recibir Welcome
    let ready_msg = ClientMessage::Ready;
    if let Ok(data) = bincode::serialize(&ready_msg) {
        println!("📤 [Red -> Servidor] Enviando READY...");
        write_half.write_all(&(data.len() as u32).to_le_bytes()).await.unwrap();
        write_half.write_all(&data).await.unwrap();
    }

    println!("✅ [Red] Handshake completo. Iniciando comunicación bidireccional...");

    // ==========================================
    // FASE 2: COMUNICACIÓN BIDIRECCIONAL
    // ==========================================

    // Task de envío de inputs (ya no necesita lógica de handshake)
    tokio::spawn(async move {
        while let Some(input) = input_rx.recv().await {
            enviar_input_packet(&mut write_half, input).await;
        }
    });

    // Loop de recepción (este ya lo tenías)
    let mut buffer = vec![0u8; 65536];
    loop {
        let mut len_buf = [0u8; 4];
        match tokio::time::timeout(std::time::Duration::from_secs(20), read_half.read_exact(&mut len_buf)).await {
            Ok(Ok(_)) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                if len > buffer.len() { buffer.resize(len, 0); }
                if let Err(e) = read_half.read_exact(&mut buffer[..len]).await {
                    println!("❌ [Red] Error leyendo datos del servidor: {:?}", e);
                    break;
                }

                if let Ok(msg) = bincode::deserialize::<ServerMessage>(&buffer[..len]) {
                    // Log para mensajes que no sean GameState (para no llenar la consola)
                    if !matches!(msg, ServerMessage::GameState { .. }) {
                        println!("📥 [Red <- Servidor] Mensaje recibido: {:?}", msg);
                    }

                    if let Err(_) = network_tx.try_send(msg) {
                        println!("⚠️ [Red] El canal de Bevy se ha cerrado");
                        break;
                    }
                }
            }
            _ => {
                println!("🔌 [Red] Timeout o error de conexión (20s sin datos)");
                break;
            }
        }
    }
}

// Antes: ...input: PlayerInput
async fn enviar_input_packet(
    write: &mut tokio::net::tcp::OwnedWriteHalf,
    msg: shared::protocol::ClientMessage
) {
    use tokio::io::AsyncWriteExt;

    if let Ok(data) = bincode::serialize(&msg) {
        // 1. Enviar longitud (u32, 4 bytes)
        let len = data.len() as u32;
        if let Err(e) = write.write_all(&len.to_le_bytes()).await {
            eprintln!("❌ Error enviando longitud: {}", e);
            return;
        }

        // 2. Enviar datos
        if let Err(e) = write.write_all(&data).await {
            eprintln!("❌ Error enviando datos: {}", e);
        }

        // Log opcional para verificar en el cliente
        if matches!(msg, ClientMessage::Input { .. }) {
            // println!("🕹️ [Cliente] Input enviado al servidor ({} bytes)", data.len());
        }
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
    if my_player_id.0.is_none() {
        return;
    }

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

    if input != previous_input.0 {
        // --- LOG DE MOVIMIENTO DETECTADO ---
        println!("🕹️ [Bevy] Cambio de input detectado. Enviando al hilo de red...");

        let msg = shared::protocol::ClientMessage::Input {
            sequence: 0,
            input: input.clone(),
        };

        if let Err(e) = input_sender.0.try_send(msg) {
            println!("⚠️ [Bevy] Error enviando input al canal: {:?}", e);
        }
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
    mut ball_q: Query<(&mut Interpolated, &mut Transform, &RemoteBall), Without<RemotePlayer>>,
    mut players_q: Query<(&mut Interpolated, &mut Transform, &RemotePlayer), (Without<RemoteBall>, Without<MainCamera>)>,
) {
    let mut rx = network_rx.0.lock().unwrap();
    let mut ball_spawned = false;
    let mut spawned_this_frame = std::collections::HashSet::new();

    // Procesar solo el último GameState si hay múltiples
    let mut last_game_state: Option<(Vec<shared::protocol::PlayerState>, shared::protocol::BallState)> = None;
    let mut messages = Vec::new();

    while let Ok(msg) = rx.try_recv() {
        messages.push(msg);
    }

    for msg in messages {
        match msg {
            ServerMessage::Welcome { player_id, .. } => {
                println!("🎉 [Bevy] Welcome recibido. Mi PlayerID es: {}", player_id);
                my_id.0 = Some(player_id);
            }
            ServerMessage::GameState { players, ball, .. } => {
                last_game_state = Some((players, ball));
            }
            _ => {}
        }
    }

    // Procesar solo el último GameState si existe
    if let Some((players, ball)) = last_game_state {
        // Actualizar Pelota
        let ball_exists = !ball_q.is_empty();
        if ball_exists {
            for (mut interp, mut transform, _) in ball_q.iter_mut() {
                interp.target_position = Vec2::new(ball.position.0, ball.position.1);
                interp.target_velocity = Vec2::new(ball.velocity.0, ball.velocity.1);
                transform.translation.x = ball.position.0;
                transform.translation.y = ball.position.1;
            }
        } else if !ball_spawned {
            ball_spawned = true;
            println!("⚽ [Bevy] Spawneando pelota visual en {:?}", ball.position);
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite { color: Color::WHITE, custom_size: Some(Vec2::splat(15.0)), ..default() },
                    transform: Transform::from_xyz(ball.position.0, ball.position.1, 1.0),
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
            for (mut interp, mut transform, rp) in players_q.iter_mut() {
                if rp.id == ps.id {
                    interp.target_position = ps.position;
                    interp.target_velocity = Vec2::new(ps.velocity.0, ps.velocity.1);
                    transform.translation.x = ps.position.x;
                    transform.translation.y = ps.position.y;
                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);
                println!("🆕 [Bevy] Spawneando jugador visual: {} (ID: {})", ps.name, ps.id);
                commands.spawn((
                    SpriteBundle {
                        sprite: Sprite {
                            color: if my_id.0 == Some(ps.id) { Color::srgb(0.2, 0.4, 1.0) } else { Color::srgb(1.0, 0.3, 0.3) },
                            custom_size: Some(Vec2::splat(45.0)),
                            ..default()
                        },
                        transform: Transform::from_xyz(ps.position.x, ps.position.y, 2.0),
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

