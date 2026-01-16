use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use clap::Parser;
use matchbox_socket::{PeerId, PeerState, WebRtcSocket};
use shared::protocol::{
    ControlMessage, GameConfig, GameDataMessage, NetworkInputType, PlayerInput, ServerMessage,
};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Parser, Debug)]
#[command(name = "Haxball Client")]
#[command(about = "Cliente del juego Haxball", long_about = None)]
struct Args {
    /// URL del servidor de señalización matchbox (ej: ws://localhost:3536)
    #[arg(short, long, default_value = "ws://127.0.0.1:3536")]
    server: String,

    /// Nombre de la sala/room en matchbox
    #[arg(short, long, default_value = "game_server")]
    room: String,

    /// Nombre del jugador
    #[arg(long, default_value = "Player")]
    name: String,
}

fn main() {
    let args = Args::parse();
    println!("🎮 Haxball Client - Iniciando...");

    let (network_tx, network_rx) = mpsc::channel();
    let (input_tx, input_rx) = mpsc::channel();

    let server_url = args.server.clone();
    let room = args.room.clone();
    let player_name = args.name.clone();

    // Hilo de red con matchbox WebRTC
    std::thread::spawn(move || {
        println!("🌐 [Red] Iniciando cliente WebRTC");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Fallo al crear Runtime de Tokio");

        rt.block_on(async {
            start_webrtc_client(server_url, room, player_name, network_tx, input_rx).await;
        });
        println!("🌐 [Red] El hilo de red HA TERMINADO");
    });

    // Bevy
    println!("🎨 [Bevy] Intentando abrir ventana...");
    App::new()
        .insert_resource(bevy::winit::WinitSettings::game())
        .insert_resource(ClearColor(Color::srgb(0.2, 0.5, 0.2))) // Fondo verde para evitar el gris
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("Haxball - {}", args.name),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .insert_resource(GameConfig::default())
        .insert_resource(NetworkReceiver(Arc::new(Mutex::new(network_rx))))
        .insert_resource(InputSender(input_tx))
        .insert_resource(MyPlayerId(None))
        .insert_resource(LoadedMap::default())
        .insert_resource(PreviousInput::default())
        .insert_resource(DoubleTapTracker {
            last_space_press: -999.0,
        })
        .add_systems(Startup, setup)
        // Lógica de red y entrada (frecuencia fija)
        .add_systems(
            FixedUpdate,
            (
                handle_input,             // Enviamos inputs al ritmo del tickrate
                process_network_messages, // Procesamos paquetes llegados
            ),
        )
        // Lógica visual y renderizado (frecuencia del monitor)
        .add_systems(
            Update,
            (
                adjust_field_for_map, // Ajusta campo y oculta líneas si hay mapa
                render_map,           // Dibuja el mapa cargado del servidor
                interpolate_entities, // Suaviza el movimiento entre posiciones de red
                camera_follow_player, // La cámara debe seguir al jugador cada frame
                camera_zoom_control,  // Control de zoom con teclas numéricas
                update_charge_bar,    // Actualiza la barra de carga de patada
                update_player_sprite, // Cambia sprite según estado de slide
                update_target_ball_position,
                update_dash_cooldown,
            ),
        )
        .run();

    println!("✅ [Bevy] App::run() ha finalizado normalmente");
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

#[derive(Resource)]
struct DoubleTapTracker {
    last_space_press: f32,
}

#[derive(Resource, Default)]
struct LoadedMap(Option<shared::map::Map>);

#[derive(Component)]
struct DefaultFieldLine;

#[derive(Component)]
struct FieldBackground;

// ============================================================================
// COMPONENTES
// ============================================================================

#[derive(Component)]
struct RemotePlayer {
    id: u32,
    kick_charge: f32,
    is_sliding: bool,
    not_interacting: bool,
    base_color: Color,
    ball_target_position: Option<Vec2>,
    stamin_charge: f32,
}

#[derive(Component)]
struct RemoteBall;

#[derive(Component)]
struct MainCamera;

#[derive(Component)]
struct Interpolated {
    target_position: Vec2,
    target_velocity: Vec2,
    target_rotation: f32,
    smoothing: f32,
}

#[derive(Component)]
struct KickChargeBar;

#[derive(Component)]
struct KickChargeBarCurveLeft;

#[derive(Component)]
struct KickChargeBarCurveRight;

#[derive(Component)]
struct DashCooldown;

#[derive(Component)]
struct PlayerSprite {
    parent_id: u32, // ID del jugador padre
}

// ============================================================================
// NETWORK CLIENT (Matchbox WebRTC)
// ============================================================================

async fn start_webrtc_client(
    signaling_url: String,
    room: String,
    player_name: String,
    network_tx: mpsc::Sender<ServerMessage>,
    mut input_rx: mpsc::Receiver<PlayerInput>,
) {
    println!(
        "🔌 [Red] Conectando a matchbox en {}/{}",
        signaling_url, room
    );

    // Crear WebRtcSocket y conectar a la room
    let room_url = format!("{}/{}", signaling_url, room);
    let (mut socket, loop_fut) = WebRtcSocket::builder(room_url)
        .add_channel(matchbox_socket::ChannelConfig::reliable()) // Canal 0: Control
        .add_channel(matchbox_socket::ChannelConfig::unreliable()) // Canal 1: GameData
        .build();

    // Spawn el loop de matchbox
    tokio::spawn(loop_fut);

    println!("✅ [Red] WebRTC socket creado, esperando conexión con el servidor...");

    // Esperar a que se conecte al menos 1 peer (el servidor) y obtener su PeerId
    let server_peer_id = loop {
        socket.update_peers();
        let mut peers = socket.connected_peers().collect::<Vec<_>>();
        if !peers.is_empty() {
            println!("🔗 [Red] Conectado al servidor!");
            break peers[0];
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    };

    // Enviar JOIN message
    let join_msg = ControlMessage::Join {
        player_name: player_name.clone(),
        input_type: NetworkInputType::Keyboard,
    };
    if let Ok(data) = bincode::serialize(&join_msg) {
        println!("📤 [Red -> Servidor] Enviando JOIN...");
        socket.channel_mut(0).send(data.into(), server_peer_id); // Canal 0 = reliable
    }

    // Loop principal: recibir mensajes y enviar inputs
    loop {
        // Recibir mensajes del servidor
        // Canal 0: Control messages (reliable)
        for (_peer_id, packet) in socket.channel_mut(0).receive() {
            if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                match msg {
                    ControlMessage::Welcome {
                        player_id,
                        game_config,
                        map,
                    } => {
                        println!("🎉 [Red] WELCOME recibido! Player ID: {}", player_id);
                        // Convertir a ServerMessage para compatibilidad con el código existente
                        let server_msg = ServerMessage::Welcome {
                            player_id,
                            game_config,
                            map,
                        };
                        let _ = network_tx.send(server_msg);

                        // Enviar READY inmediatamente
                        let ready_msg = ControlMessage::Ready;
                        if let Ok(data) = bincode::serialize(&ready_msg) {
                            println!("📤 [Red -> Servidor] Enviando READY...");
                            socket.channel_mut(0).send(data.into(), server_peer_id);
                            // Canal 0 = reliable
                        }
                    }
                    _ => {}
                }
            }
        }

        // Canal 1: GameData messages (unreliable)
        for (_peer_id, packet) in socket.channel_mut(1).receive() {
            if let Ok(msg) = bincode::deserialize::<GameDataMessage>(&packet) {
                match msg {
                    GameDataMessage::GameState {
                        tick,
                        timestamp,
                        players,
                        ball,
                    } => {
                        // Convertir a ServerMessage
                        let server_msg = ServerMessage::GameState {
                            tick,
                            timestamp,
                            players,
                            ball,
                        };
                        let _ = network_tx.send(server_msg);
                    }
                    GameDataMessage::Pong {
                        client_timestamp,
                        server_timestamp,
                    } => {
                        let server_msg = ServerMessage::Pong {
                            client_timestamp,
                            server_timestamp,
                        };
                        let _ = network_tx.send(server_msg);
                    }
                    _ => {}
                }
            }
        }

        // Enviar inputs desde Bevy
        while let Ok(input) = input_rx.try_recv() {
            let input_msg = GameDataMessage::Input { sequence: 0, input };
            if let Ok(data) = bincode::serialize(&input_msg) {
                socket.channel_mut(1).send(data.into(), server_peer_id); // Canal 1 = unreliable
            }
        }

        // Pequeña pausa
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }
}

// ============================================================================
// GAME SYSTEMS
// ============================================================================

fn setup(mut commands: Commands, config: Res<GameConfig>, input_sender: Res<InputSender>) {
    // Cámara con zoom ajustado para mejor visualización del mapa
    commands.spawn((
        Camera2dBundle {
            projection: bevy::render::camera::OrthographicProjection {
                scale: 1.3, // Reducido de 2.0 para ver el campo más grande
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 999.0),
            ..default()
        },
        MainCamera,
    ));

    // El Campo de Juego (Césped) - Color verde de RustBall
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.2, 0.5, 0.2), // RGB(51, 127, 51) - Verde RustBall
                custom_size: Some(Vec2::new(config.arena_width, config.arena_height)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, -10.0),
            ..default()
        },
        FieldBackground,
    ));

    // Líneas blancas del campo (bordes) - igual que RustBall (z = 0.0)
    let thickness = 5.0;
    let w = config.arena_width;
    let h = config.arena_height;

    // Top
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(w + thickness, thickness)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, h / 2.0, 0.0),
            ..default()
        },
        DefaultFieldLine,
    ));

    // Bottom
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(w + thickness, thickness)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, -h / 2.0, 0.0),
            ..default()
        },
        DefaultFieldLine,
    ));

    // Left
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(thickness, h + thickness)),
                ..default()
            },
            transform: Transform::from_xyz(-w / 2.0, 0.0, 0.0),
            ..default()
        },
        DefaultFieldLine,
    ));

    // Right
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(thickness, h + thickness)),
                ..default()
            },
            transform: Transform::from_xyz(w / 2.0, 0.0, 0.0),
            ..default()
        },
        DefaultFieldLine,
    ));

    // El mensaje Ready ahora se envía automáticamente en el thread de red después de recibir Welcome

    println!("✅ Cliente configurado y campo listo");
}

// Resource para trackear el input anterior
#[derive(Resource, Default)]
struct PreviousInput(PlayerInput);

fn handle_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_sender: Res<InputSender>,
    my_player_id: Res<MyPlayerId>,
    mut previous_input: ResMut<PreviousInput>,
    mut double_tap: ResMut<DoubleTapTracker>,
    time: Res<Time>,
) {
    if my_player_id.0.is_none() {
        return;
    }

    // Detectar doble tap de Space
    let current_time = time.elapsed_seconds();
    let double_tap_window = 0.3; // 300ms para doble tap
    let mut dash_detected = false;

    if keyboard.just_pressed(KeyCode::Space) {
        let time_since_last = current_time - double_tap.last_space_press;

        if time_since_last < double_tap_window {
            dash_detected = true;
            println!("🏃 [Cliente] Doble tap detectado! Enviando slide=true");
        }

        double_tap.last_space_press = current_time;
    }

    // Mapeo de teclas EXACTO de RustBall
    let input = PlayerInput {
        move_up: keyboard.pressed(KeyCode::ArrowUp),
        move_down: keyboard.pressed(KeyCode::ArrowDown),
        move_left: keyboard.pressed(KeyCode::ArrowLeft),
        move_right: keyboard.pressed(KeyCode::ArrowRight),
        kick: keyboard.pressed(KeyCode::KeyS),
        curve_left: keyboard.pressed(KeyCode::KeyD),
        curve_right: keyboard.pressed(KeyCode::KeyA),
        stop_interact: keyboard.pressed(KeyCode::ShiftLeft),
        sprint: keyboard.pressed(KeyCode::Space),
        dash: dash_detected,
        slide: keyboard.pressed(KeyCode::ControlLeft),
    };

    // Enviamos input siempre, no solo cuando cambia (para mantener estado)
    // El canal unreliable de WebRTC puede perder paquetes, así que enviamos constantemente
    if let Err(e) = input_sender.0.send(input.clone()) {
        println!("⚠️ [Bevy] Error enviando input al canal: {:?}", e);
    }
    previous_input.0 = input;
}

fn process_network_messages(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    config: Res<GameConfig>,
    network_rx: Res<NetworkReceiver>,
    mut my_id: ResMut<MyPlayerId>,
    mut loaded_map: ResMut<LoadedMap>,
    mut ball_q: Query<(&mut Interpolated, &mut Transform, &RemoteBall), Without<RemotePlayer>>,
    mut players_q: Query<
        (
            &mut Interpolated,
            &mut Transform,
            &mut RemotePlayer,
            &mut Collider,
        ),
        (Without<RemoteBall>, Without<MainCamera>),
    >,
) {
    let mut rx = network_rx.0.lock().unwrap();
    let mut spawned_this_frame = std::collections::HashSet::new();

    // Procesar solo el último GameState si hay múltiples
    let mut last_game_state: Option<(
        Vec<shared::protocol::PlayerState>,
        shared::protocol::BallState,
    )> = None;
    let mut messages = Vec::new();

    while let Ok(msg) = rx.try_recv() {
        messages.push(msg);
    }

    for msg in messages {
        match msg {
            ServerMessage::Welcome {
                player_id,
                game_config,
                map,
            } => {
                println!("🎉 [Bevy] Welcome recibido. Mi PlayerID es: {}", player_id);
                my_id.0 = Some(player_id);

                // Almacenar mapa si fue enviado
                if let Some(received_map) = map {
                    println!("📦 [Bevy] Mapa recibido: {}", received_map.name);
                    println!(
                        "   Dimensiones: width={:?}, height={:?}",
                        received_map.width, received_map.height
                    );
                    println!(
                        "   BG: width={:?}, height={:?}",
                        received_map.bg.width, received_map.bg.height
                    );
                    println!(
                        "   Vértices: {}, Segmentos: {}, Discos: {}",
                        received_map.vertexes.len(),
                        received_map.segments.len(),
                        received_map.discs.len()
                    );
                    loaded_map.0 = Some(received_map);
                } else {
                    println!("🏟️  [Bevy] Usando arena por defecto");
                }
            }
            ServerMessage::GameState {
                players,
                ball,
                tick,
                ..
            } => {
                // Log solo el primer GameState recibido
                if tick == 1 {
                    println!("📥 [Bevy] Primer GameState recibido: {} jugadores, pelota en ({:.0}, {:.0})",
                        players.len(), ball.position.0, ball.position.1);
                }
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
        } else {
            println!("⚽ [Bevy] Spawneando pelota visual en {:?}", ball.position);
            // Igual que RustBall: usar textura con children
            commands
                .spawn((
                    SpatialBundle {
                        transform: Transform::from_xyz(ball.position.0, ball.position.1, 0.0),
                        ..default()
                    },
                    RemoteBall,
                    Collider::ball(config.ball_radius), // Para debug rendering
                    Interpolated {
                        target_position: Vec2::new(ball.position.0, ball.position.1),
                        target_velocity: Vec2::new(ball.velocity.0, ball.velocity.1),
                        target_rotation: 0.0,
                        smoothing: 20.0,
                    },
                ))
                .with_children(|parent| {
                    parent.spawn(SpriteBundle {
                        texture: asset_server.load("ball.png"),
                        sprite: Sprite {
                            custom_size: Some(Vec2::splat(config.ball_radius * 2.0)),
                            ..default()
                        },
                        transform: Transform::from_xyz(0.0, 0.0, 1.0),
                        ..default()
                    });
                });
        }

        // Actualizar Jugadores
        for ps in players {
            let mut found = false;
            for (mut interp, mut transform, mut rp, mut collider) in players_q.iter_mut() {
                if rp.id == ps.id {
                    interp.target_position = ps.position;
                    interp.target_velocity = Vec2::new(ps.velocity.0, ps.velocity.1);
                    interp.target_rotation = ps.rotation;
                    transform.translation.x = ps.position.x;
                    transform.translation.y = ps.position.y;
                    rp.kick_charge = ps.kick_charge;
                    rp.is_sliding = ps.is_sliding;
                    rp.ball_target_position = ps.ball_target_position;
                    rp.stamin_charge = ps.stamin_charge;

                    // Actualizar collider según estado de slide
                    if ps.is_sliding {
                        // Cápsula para slide (igual que servidor)
                        *collider = Collider::capsule_y(15.0, config.sphere_radius);
                        transform.rotation = Quat::from_rotation_z(ps.rotation);
                    } else {
                        // Esfera normal
                        *collider = Collider::ball(config.sphere_radius);
                    }

                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);
                println!(
                    "🆕 [Bevy] Spawneando jugador visual: {} (ID: {})",
                    ps.name, ps.id
                );

                // Generar un color basado en el ID (rápido y efectivo)
                let r = ((ps.id * 123) % 255) as f32 / 255.0;
                let g = ((ps.id * 456) % 255) as f32 / 255.0;
                let b = ((ps.id * 789) % 255) as f32 / 255.0;
                let player_color = Color::srgb(r, g, b);

                // Igual que RustBall: usar textura con children
                commands
                    .spawn((
                        SpatialBundle {
                            transform: Transform::from_xyz(ps.position.x, ps.position.y, 0.0),
                            ..default()
                        },
                        RemotePlayer {
                            id: ps.id,
                            kick_charge: ps.kick_charge,
                            is_sliding: ps.is_sliding,
                            not_interacting: ps.not_interacting,
                            base_color: player_color,
                            ball_target_position: ps.ball_target_position,
                            stamin_charge: ps.stamin_charge,
                        },
                        Collider::ball(config.sphere_radius), // Para debug rendering
                        Interpolated {
                            target_position: ps.position,
                            target_velocity: Vec2::new(ps.velocity.0, ps.velocity.1),
                            target_rotation: ps.rotation,
                            smoothing: 15.0,
                        },
                    ))
                    .with_children(|parent| {
                        // Sprite del jugador
                        parent.spawn((
                            SpriteBundle {
                                texture: asset_server.load("player.png"),
                                sprite: Sprite {
                                    color: player_color,
                                    custom_size: Some(Vec2::splat(config.sphere_radius * 2.0)),
                                    ..default()
                                },
                                transform: Transform::from_xyz(0.0, 0.0, 1.0),
                                ..default()
                            },
                            PlayerSprite { parent_id: ps.id },
                        ));

                        // Barra de carga de patada
                        parent.spawn((
                            KickChargeBar,
                            SpriteBundle {
                                sprite: Sprite {
                                    color: Color::srgb(1.0, 0.0, 0.0),
                                    custom_size: Some(Vec2::new(0.0, 5.0)),
                                    anchor: bevy::sprite::Anchor::CenterLeft,
                                    ..default()
                                },
                                //transform: Transform::from_xyz(-25.0, 60.0, 30.0),
                                transform: Transform::from_xyz(-5.0, 0.0, 30.0),
                                ..default()
                            },
                        ));

                        let angle = 25.0f32.to_radians();

                        // Barra de carga de patada a la izquierda
                        parent.spawn((
                            KickChargeBarCurveLeft,
                            SpriteBundle {
                                sprite: Sprite {
                                    color: Color::srgb(1.0, 0.0, 0.0),
                                    custom_size: Some(Vec2::new(0.0, 5.0)),
                                    anchor: bevy::sprite::Anchor::CenterLeft,
                                    ..default()
                                },
                                transform: Transform {
                                    translation: Vec3::new(0.0, -10.0, 30.0),
                                    // Rotación hacia la izquierda (positiva en el eje Z)
                                    rotation: Quat::from_rotation_z(-angle),
                                    ..default()
                                },
                                ..default()
                            },
                        ));

                        // Barra de carga de patada a la derecha
                        parent.spawn((
                            KickChargeBarCurveRight,
                            SpriteBundle {
                                sprite: Sprite {
                                    color: Color::srgb(1.0, 0.0, 0.0),
                                    custom_size: Some(Vec2::new(0.0, 5.0)),
                                    anchor: bevy::sprite::Anchor::CenterLeft,
                                    ..default()
                                },
                                transform: Transform {
                                    translation: Vec3::new(0.0, 10.0, 30.0),
                                    // Rotación hacia la derecha (negativa en el eje Z)
                                    rotation: Quat::from_rotation_z(angle),
                                    ..default()
                                },
                                ..default()
                            },
                        ));

                        let angle2 = 90.0f32.to_radians();

                        // Barra de temporizadora de regate
                        parent.spawn((
                            DashCooldown,
                            SpriteBundle {
                                sprite: Sprite {
                                    color: Color::srgb(1.0, 0.0, 0.0),
                                    custom_size: Some(Vec2::new(0.0, 5.0)),
                                    anchor: bevy::sprite::Anchor::CenterLeft,
                                    ..default()
                                },
                                transform: Transform {
                                    translation: Vec3::new(-10.0, -15.0, 30.0),
                                    // Rotación hacia la derecha (negativa en el eje Z)
                                    rotation: Quat::from_rotation_z(angle2),
                                    ..default()
                                },
                                ..default()
                            },
                        ));
                    });
            }
        }
    }
}

// 3. Sistema de interpolación (Actualizado)
fn interpolate_entities(time: Res<Time>, mut q: Query<(&mut Transform, &Interpolated)>) {
    let dt = time.delta_seconds();
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

// Sistema de control de zoom con teclas numéricas
fn camera_zoom_control(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut camera: Query<&mut bevy::render::camera::OrthographicProjection, With<MainCamera>>,
) {
    if let Ok(mut projection) = camera.get_single_mut() {
        let mut new_scale = None;

        // Teclas 1-9 para diferentes niveles de zoom
        if keyboard.just_pressed(KeyCode::Digit1) {
            new_scale = Some(0.5); // Muy cerca
        } else if keyboard.just_pressed(KeyCode::Digit2) {
            new_scale = Some(0.75);
        } else if keyboard.just_pressed(KeyCode::Digit3) {
            new_scale = Some(1.0); // Normal
        } else if keyboard.just_pressed(KeyCode::Digit4) {
            new_scale = Some(1.3);
        } else if keyboard.just_pressed(KeyCode::Digit5) {
            new_scale = Some(1.5);
        } else if keyboard.just_pressed(KeyCode::Digit6) {
            new_scale = Some(2.0); // Lejos
        } else if keyboard.just_pressed(KeyCode::Digit7) {
            new_scale = Some(2.5);
        } else if keyboard.just_pressed(KeyCode::Digit8) {
            new_scale = Some(3.0);
        } else if keyboard.just_pressed(KeyCode::Digit9) {
            new_scale = Some(4.0); // Muy lejos
        }

        if let Some(scale) = new_scale {
            projection.scale = scale;
            println!("📷 Zoom ajustado a: {:.1}x", scale);
        }
    }
}

fn update_charge_bar(
    player_query: Query<(&RemotePlayer, &Children)>,
    previous_input: Res<PreviousInput>, // Usamos Res si no vas a modificar el input
    // Una sola query mutable para el Sprite evita el conflicto B0001
    mut sprite_query: Query<&mut Sprite>,
    // Queries de solo lectura para identificar qué tipo de barra es cada hijo
    bar_main_q: Query<Entity, With<KickChargeBar>>,
    bar_left_q: Query<Entity, With<KickChargeBarCurveLeft>>,
    bar_right_q: Query<Entity, With<KickChargeBarCurveRight>>,
) {
    let max_width = 50.0;

    for (player, children) in player_query.iter() {
        for &child in children.iter() {
            // Intentamos obtener el sprite del hijo
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                // 1. Caso: Barra Principal
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * player.kick_charge, 5.0));
                    sprite.color = Color::srgb(1.0, 1.0 - player.kick_charge, 0.0);
                }
                // 2. Caso: Curva Izquierda
                else if bar_left_q.contains(child) {
                    let coeficient = if previous_input.0.curve_left {
                        0.5
                    } else {
                        0.0
                    };
                    sprite.custom_size =
                        Some(Vec2::new(max_width * player.kick_charge * coeficient, 5.0));
                    sprite.color = Color::srgb(0.0, 1.0, 1.0); // Color distinto para debug si quieres
                }
                // 3. Caso: Curva Derecha
                else if bar_right_q.contains(child) {
                    let coeficient = if previous_input.0.curve_right {
                        0.5
                    } else {
                        0.0
                    };
                    sprite.custom_size =
                        Some(Vec2::new(max_width * player.kick_charge * coeficient, 5.0));
                    sprite.color = Color::srgb(0.0, 1.0, 1.0);
                }
            }
        }
    }
}

fn update_dash_cooldown(
    player_query: Query<(&RemotePlayer, &Children)>,
    // Una sola query mutable para el Sprite evita el conflicto B0001
    mut sprite_query: Query<&mut Sprite>,
    // Queries de solo lectura para identificar qué tipo de barra es cada hijo
    bar_main_q: Query<Entity, With<DashCooldown>>,
) {
    let max_width = 30.0;

    for (player, children) in player_query.iter() {
        for &child in children.iter() {
            // Intentamos obtener el sprite del hijo
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                // 1. Caso: Barra Principal
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * player.stamin_charge, 5.0));

                    sprite.color =
                        Color::srgb(1.0, 0.5 * player.stamin_charge, player.stamin_charge);
                }
            }
        }
    }
}

fn update_player_sprite(
    player_query: Query<&RemotePlayer>,
    mut sprite_query: Query<(&PlayerSprite, &mut Handle<Image>, &mut Sprite)>,
    asset_server: Res<AssetServer>,
    config: Res<GameConfig>,
) {
    for (player_sprite, mut texture, mut sprite) in sprite_query.iter_mut() {
        // Buscamos al jugador padre para obtener su color base y estado
        if let Some(player) = player_query
            .iter()
            .find(|p| p.id == player_sprite.parent_id)
        {
            // 1. Gestionar Textura según Slide
            if player.is_sliding {
                *texture = asset_server.load("player_slide.png");
                sprite.custom_size = Some(Vec2::new(
                    config.sphere_radius * 2.0,
                    config.sphere_radius * 2.5,
                ));
            } else {
                *texture = asset_server.load("player.png");
                sprite.custom_size = Some(Vec2::splat(config.sphere_radius * 2.0));
            }

            // 2. APLICAR COLOR Y TRANSPARENCIA
            // Si el modo stop_interact está activo, usamos alfa 0.3, si no 1.0
            let alpha = if player.not_interacting { 0.3 } else { 1.0 };

            // Aplicamos el color base que guardamos al spawnear con el nuevo alfa
            sprite.color = player.base_color.with_alpha(alpha);
        }
    }
}

// Sistema para ocultar líneas por defecto y ajustar campo cuando hay mapa
fn adjust_field_for_map(
    loaded_map: Res<LoadedMap>,
    mut default_lines: Query<&mut Visibility, With<DefaultFieldLine>>,
    mut field_bg: Query<
        (&mut Sprite, &mut Transform),
        (With<FieldBackground>, Without<DefaultFieldLine>),
    >,
) {
    if loaded_map.is_changed() {
        if let Some(map) = &loaded_map.0 {
            // Hay mapa: ocultar líneas por defecto
            for mut visibility in default_lines.iter_mut() {
                *visibility = Visibility::Hidden;
            }

            // Ajustar tamaño del campo según dimensiones del mapa
            // Usar primero las dimensiones del nivel raíz, luego las del bg como fallback
            let width = map.width.or(map.bg.width);
            let height = map.height.or(map.bg.height);

            if let (Some(w), Some(h)) = (width, height) {
                if let Ok((mut sprite, _transform)) = field_bg.get_single_mut() {
                    sprite.custom_size = Some(Vec2::new(w, h));
                    println!("🎨 Campo ajustado a dimensiones del mapa: {}x{}", w, h);
                }
            } else {
                println!("⚠️  Mapa sin dimensiones definidas, usando tamaño por defecto");
            }
        } else {
            // No hay mapa: mostrar líneas por defecto
            for mut visibility in default_lines.iter_mut() {
                *visibility = Visibility::Visible;
            }
        }
    }
}

// Sistema para renderizar el mapa usando Gizmos
fn render_map(mut gizmos: Gizmos, loaded_map: Res<LoadedMap>) {
    let Some(map) = &loaded_map.0 else {
        return; // No hay mapa cargado
    };

    // Colores según tipo de interacción
    let ball_color = Color::srgb(0.3, 0.7, 1.0); // Azul claro - solo pelota
    let player_color = Color::srgb(0.3, 1.0, 0.5); // Verde claro - solo jugadores
    let decorative_color = Color::srgb(0.5, 0.5, 0.5); // Gris - decorativo sin física
    let vertex_color = Color::srgb(1.0, 0.2, 0.2); // Rojo para vértices
    let disc_color = Color::srgb(0.7, 0.7, 0.7); // Gris para discos

    // Dibujar vértices (puntos de interacción)
    for (_i, vertex) in map.vertexes.iter().enumerate() {
        let pos = Vec3::new(vertex.x, vertex.y, 6.0); // z=6 para que esté encima
        gizmos.circle(pos, Dir3::Z, 3.0, vertex_color); // Radio pequeño 3.0
    }

    // Dibujar segmentos (paredes)
    for segment in &map.segments {
        // SKIP si el segmento es invisible (vis=false)
        if !segment.is_visible() {
            continue;
        }

        if segment.v0 >= map.vertexes.len() || segment.v1 >= map.vertexes.len() {
            continue; // Saltar segmentos inválidos
        }

        let v0 = &map.vertexes[segment.v0];
        let v1 = &map.vertexes[segment.v1];

        let p0 = Vec3::new(v0.x, v0.y, 5.0); // z=5 para que esté encima del campo
        let p1 = Vec3::new(v1.x, v1.y, 5.0);

        // Determinar color según cMask (tipo de colisión)
        let line_color = if let Some(cmask) = &segment.c_mask {
            if cmask.is_empty() || cmask.iter().any(|m| m.is_empty()) {
                decorative_color // Sin colisión
            } else if cmask.iter().any(|m| m == "ball")
                && !cmask.iter().any(|m| m == "red" || m == "blue")
            {
                ball_color // Solo pelota
            } else if cmask.iter().any(|m| m == "red" || m == "blue") {
                player_color // Solo jugadores
            } else {
                decorative_color // Otro caso sin interacción
            }
        } else {
            decorative_color // Sin cMask = decorativo
        };

        // Verificar si el segmento es curvo
        let curve_factor = segment.curve.or(segment.curve_f).unwrap_or(0.0);

        if curve_factor.abs() < 0.01 {
            // Segmento recto
            gizmos.line(p0, p1, line_color);
        } else {
            // Segmento curvo - aproximar con más líneas para mejor visualización
            let num_segments = 24; // Más segmentos para curvas más suaves
            let points = approximate_curve_for_rendering(
                Vec2::new(v0.x, v0.y),
                Vec2::new(v1.x, v1.y),
                curve_factor,
                num_segments,
            );

            // Dibujar líneas conectadas
            for i in 0..points.len() - 1 {
                gizmos.line(
                    Vec3::new(points[i].x, points[i].y, 5.0),
                    Vec3::new(points[i + 1].x, points[i + 1].y, 5.0),
                    line_color,
                );
            }
        }
    }

    // Dibujar discos (obstáculos circulares)
    for disc in &map.discs {
        let pos = Vec3::new(disc.pos[0], disc.pos[1], 5.0);
        gizmos.circle(pos, Dir3::Z, disc.radius, disc_color);
    }
}

fn update_target_ball_position(mut gizmos: Gizmos, player_query: Query<&RemotePlayer>) {
    for player in player_query.iter() {
        let Some(b_target_pos) = player.ball_target_position else {
            return;
        };
        println!("ball target pos {}", b_target_pos);

        let disc_color = Color::srgb(0.7, 0.7, 0.7);

        gizmos.circle_2d(b_target_pos, 3.0, disc_color);
    }
}

// Función auxiliar para aproximar curvas (HaxBall curve format)
fn approximate_curve_for_rendering(
    p0: Vec2,
    p1: Vec2,
    curve: f32,
    num_segments: usize,
) -> Vec<Vec2> {
    let mut points = Vec::with_capacity(num_segments + 1);

    let chord = p0.distance(p1);
    let radius = curve.abs();

    // Si el radio es muy pequeño o inválido, retornar línea recta
    if radius < chord / 2.0 {
        points.push(p0);
        points.push(p1);
        return points;
    }

    // Calcular el ángulo subtendido por la cuerda
    let half_angle = (chord / (2.0 * radius)).asin();
    let total_angle = 2.0 * half_angle;

    // Punto medio de la cuerda
    let midpoint = (p0 + p1) * 0.5;

    // Vector de p0 a p1
    let chord_vec = p1 - p0;

    // Vector perpendicular (normalizado)
    let perp = Vec2::new(-chord_vec.y, chord_vec.x).normalize();

    // Distancia del centro a la cuerda
    let height = (radius * radius - (chord / 2.0) * (chord / 2.0)).sqrt();

    // Centro del círculo (curva positiva = perp positivo, negativa = perp negativo)
    let center = if curve > 0.0 {
        midpoint + perp * height
    } else {
        midpoint - perp * height
    };

    // Ángulo inicial (de center a p0)
    let start_angle = (p0.y - center.y).atan2(p0.x - center.x);

    // Determinar dirección de barrido
    let angle_step = if curve > 0.0 {
        -total_angle / num_segments as f32
    } else {
        total_angle / num_segments as f32
    };

    // Generar puntos
    for i in 0..=num_segments {
        let angle = start_angle + angle_step * i as f32;
        let point = Vec2::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        points.push(point);
    }

    points
}
