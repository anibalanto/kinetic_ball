use bevy::math::VectorSpace;
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use clap::Parser;
use matchbox_socket::{PeerId, PeerState, WebRtcSocket};
use shared::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

mod input;
mod map;

use input::{GameAction, InputSource, NetworkInputSource};

/// HaxBall Server - Servidor de física para juego de fútbol
#[derive(Parser, Debug)]
#[command(name = "haxball-server")]
#[command(about = "Servidor de física para HaxBall multiplayer", long_about = None)]
struct Cli {
    /// Ruta al archivo de mapa (.hbs, .json, .json5)
    #[arg(short, long, value_name = "FILE")]
    map: Option<String>,

    /// Factor de escala para el mapa (ej: 2.0 = mapa 2x más grande)
    #[arg(short, long, default_value = "1.0")]
    scale: f32,

    /// Listar mapas disponibles en el directorio maps/
    #[arg(short, long)]
    list_maps: bool,

    /// Puerto del servidor de juego (WebRTC data channels)
    #[arg(short, long, default_value = "9000")]
    port: u16,

    /// URL del servidor de señalización matchbox (ej: ws://localhost:3536 o wss://matchbox.ejemplo.com)
    #[arg(long, default_value = "ws://127.0.0.1:3536")]
    signaling_url: String,

    /// Nombre de la sala/room en matchbox
    #[arg(long, default_value = "game_server")]
    room: String,
}

fn main() {
    let cli = Cli::parse();

    // Si se solicita listar mapas, mostrar y salir
    if cli.list_maps {
        println!("📂 Mapas disponibles en maps/:\n");
        let maps = map::list_available_maps("maps");
        if maps.is_empty() {
            println!("   (No se encontraron mapas)");
        } else {
            for (i, map_path) in maps.iter().enumerate() {
                let name = map_path.file_name().unwrap().to_string_lossy();
                println!("   {}. {}", i + 1, name);
            }
        }
        println!("\nUso: cargo run --release --bin server -- --map maps/<nombre>");
        return;
    }

    println!("🎮 Haxball Server - Iniciando...");

    // Configurar GameConfig con el mapa desde CLI
    let (game_config, loaded_map) = if let Some(map_path) = cli.map {
        println!("🗺️  Cargando mapa: {}", map_path);

        // Intentar cargar el mapa
        let loaded_map = match map::load_map(&map_path) {
            Ok(mut m) => {
                println!("   ✅ Mapa cargado: {}", m.name);

                // Aplicar escala si es diferente de 1.0
                if (cli.scale - 1.0).abs() > 0.01 {
                    println!("   📏 Aplicando escala: {}x", cli.scale);
                    m.scale(cli.scale);
                }

                Some(m)
            }
            Err(e) => {
                eprintln!("   ⚠️  Error cargando mapa: {}", e);
                eprintln!("   Continuando con arena por defecto");
                None
            }
        };

        let config = GameConfig {
            map_path: Some(map_path),
            use_default_walls: loaded_map.is_none(),
            ..Default::default()
        };

        (config, loaded_map)
    } else {
        println!("🏟️  Usando arena por defecto");
        (GameConfig::default(), None)
    };

    // IMPORTANTE: Se requiere ejecutar matchbox_server (o tener uno accesible en la URL configurada)
    // cargo install matchbox_server
    // matchbox_server

    println!(
        "⚠️  Asegúrate de tener matchbox_server accesible en: {}",
        cli.signaling_url
    );
    println!("   Para local: matchbox_server");

    let (network_tx, network_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel();

    // Clonar loaded_map para usarlo en ambos lugares
    let network_state = Arc::new(Mutex::new(NetworkState {
        next_player_id: 1,
        game_config: game_config.clone(),
        map: loaded_map.clone(),
    }));

    // Iniciar servidor WebRTC (se conecta a matchbox como peer)
    let signaling_url = cli.signaling_url.clone();
    let room = cli.room.clone();
    std::thread::spawn(move || {
        start_webrtc_server(network_tx, network_state, signaling_url, room, outgoing_rx);
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
        .insert_resource(game_config)
        .insert_resource(NetworkReceiver(Arc::new(Mutex::new(network_rx))))
        .insert_resource(NetworkSender(outgoing_tx))
        .insert_resource(LoadedMap(loaded_map.clone()))
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
                update_input_manager,
                process_network_messages,
                detect_slide,
                execute_slide,
                move_players,
                handle_collision_player,
                look_at_ball,
                charge_kick,
                charge_curve,
                kick_ball,
                apply_magnus_effect,
                attract_ball,
                dash_first_touch_ball,
                update_ball_damping,
                broadcast_game_state,
                recover_stamin,
            )
                .chain(),
        )
        .run();
}

// ============================================================================
// RECURSOS Y COMPONENTES
// ============================================================================

#[derive(Resource)]
struct NetworkReceiver(Arc<Mutex<mpsc::Receiver<NetworkEvent>>>);

#[derive(Resource)]
struct NetworkSender(mpsc::Sender<OutgoingMessage>);

#[derive(Resource)]
struct GameTick(u32);

#[derive(Resource)]
struct BroadcastTimer(Timer);

#[derive(Resource)]
struct LoadedMap(Option<shared::map::Map>);

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

    pub fn tick(&mut self) {
        for source in self.sources.values_mut() {
            InputSource::update(source);
        }
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
    peer_id: PeerId, // Matchbox peer ID para enviar mensajes
    pub is_ready: bool,

    not_interacting: bool,
    // Barrida/Slide
    is_sliding: bool,
    slide_direction: Vec2,
    slide_timer: f32,

    ball_target_position: Option<Vec2>,
    stamin: f32,
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
    game_config: GameConfig,
    map: Option<shared::map::Map>,
}

enum NetworkEvent {
    NewPlayer {
        id: u32,
        name: String,
        peer_id: PeerId, // Matchbox peer ID
    },
    PlayerInput {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
        input: PlayerInput,
    },
    PlayerDisconnected {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
    },
    PlayerReady {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
    },
}

/// Mensajes salientes del servidor a los clientes
enum OutgoingMessage {
    /// Enviar a un peer específico por un canal específico
    ToOne {
        peer_id: PeerId,
        channel: usize, // 0 = reliable, 1 = unreliable
        data: Vec<u8>,
    },
    /// Enviar a todos los peers conectados
    Broadcast { channel: usize, data: Vec<u8> },
}

// ============================================================================
// NETWORK SERVER - MATCHBOX WEBRTC
// ============================================================================

fn start_webrtc_server(
    event_tx: mpsc::Sender<NetworkEvent>,
    state: Arc<Mutex<NetworkState>>,
    signaling_url: String,
    room: String,
    outgoing_rx: mpsc::Receiver<OutgoingMessage>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("No se pudo crear el runtime de Tokio");

    rt.block_on(async {
        println!(
            "🌐 Server connecting to matchbox at {}/{}",
            signaling_url, room
        );

        // Crear WebRtcSocket y conectar a la room
        let room_url = format!("{}/{}", signaling_url, room);
        let (mut socket, loop_fut) = WebRtcSocket::builder(room_url)
            .add_channel(matchbox_socket::ChannelConfig::reliable()) // Canal 0: Control (reliable)
            .add_channel(matchbox_socket::ChannelConfig::unreliable()) // Canal 1: GameData (unreliable)
            .build();

        // Spawn el loop de matchbox (maneja la señalización)
        tokio::spawn(loop_fut);

        println!("✅ Server WebRTC socket ready, waiting for peers...");

        // Loop principal: manejar eventos de peers y mensajes
        loop {
            // Procesar eventos de conexión/desconexión de peers
            for (peer_id, peer_state) in socket.update_peers() {
                match peer_state {
                    PeerState::Connected => {
                        println!("🔗 Peer connected: {:?}", peer_id);
                        // No asignamos player_id aquí, esperamos el mensaje JOIN
                    }
                    PeerState::Disconnected => {
                        println!("🔌 Peer disconnected: {:?}", peer_id);
                        let _ = event_tx.send(NetworkEvent::PlayerDisconnected { peer_id });
                    }
                }
            }

            // Recibir mensajes del canal 0 (reliable - control)
            for (peer_id, packet) in socket.channel_mut(0).receive() {
                if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                    handle_control_message_typed(&event_tx, &state, peer_id, msg);
                }
            }

            // Recibir mensajes del canal 1 (unreliable - game data)
            for (peer_id, packet) in socket.channel_mut(1).receive() {
                if let Ok(msg) = bincode::deserialize::<GameDataMessage>(&packet) {
                    handle_game_data_message_typed(&event_tx, peer_id, msg);
                }
            }

            // Enviar mensajes salientes desde Bevy a los clientes
            while let Ok(outgoing) = outgoing_rx.try_recv() {
                match outgoing {
                    OutgoingMessage::ToOne {
                        peer_id,
                        channel,
                        data,
                    } => {
                        socket.channel_mut(channel).send(data.into(), peer_id);
                    }
                    OutgoingMessage::Broadcast { channel, data } => {
                        // Colectar peer_ids primero para evitar borrow conflict
                        let peers: Vec<_> = socket.connected_peers().collect();
                        for peer_id in peers {
                            socket
                                .channel_mut(channel)
                                .send(data.clone().into(), peer_id);
                        }
                    }
                }
            }

            // Pequeña pausa para no saturar el CPU
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    });
}

fn peer_id_to_u32(peer_id: PeerId) -> u32 {
    // Convertir PeerId (UUID) a u32 usando hash
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    peer_id.hash(&mut hasher);
    hasher.finish() as u32
}

fn handle_control_message_typed(
    event_tx: &mpsc::Sender<NetworkEvent>,
    state: &Arc<Mutex<NetworkState>>,
    peer_id: PeerId,
    msg: ControlMessage,
) {
    match msg {
        ControlMessage::Join {
            player_name,
            input_type,
        } => {
            let (id, config, map) = {
                let mut s = state.lock().unwrap();
                let id = s.next_player_id;
                s.next_player_id += 1;
                (id, s.game_config.clone(), s.map.clone())
            };

            println!("🎮 Player {} joined: {}", id, player_name);

            // Enviar Welcome de vuelta (esto lo maneja broadcast_game_state por ahora)
            // TODO: Implementar envío directo de Welcome a este peer

            let _ = event_tx.send(NetworkEvent::NewPlayer {
                id,
                name: player_name,
                peer_id,
            });
        }
        ControlMessage::Ready => {
            println!("✅ Player with peer_id {:?} ready", peer_id);
            let _ = event_tx.send(NetworkEvent::PlayerReady { peer_id });
        }
        _ => {
            // Otros mensajes de control del servidor no deberían venir del cliente
        }
    }
}

fn handle_game_data_message_typed(
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: PeerId,
    msg: GameDataMessage,
) {
    match msg {
        GameDataMessage::Input { sequence, input } => {
            let _ = event_tx.send(NetworkEvent::PlayerInput { peer_id, input });
        }
        GameDataMessage::Ping { timestamp } => {
            // TODO: Responder con Pong
        }
        _ => {
            // Otros mensajes del servidor no deberían venir del cliente
        }
    }
}

/* CÓDIGO ANTIGUO DE TOKIO - COMENTADO
fn start_network_server(
    event_tx: mpsc::Sender<NetworkEvent>,
    state: Arc<Mutex<NetworkState>>,
    port: u16,
) {
    // Creamos un runtime de Tokio dedicado para la red
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("No se pudo crear el runtime de Tokio");

    rt.block_on(async {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr)
            .await
            .unwrap_or_else(|_| panic!("No se pudo enlazar el servidor al puerto {}", port));

        println!("🌐 Servidor escuchando en {}", addr);

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
*/

/* HANDLE_CLIENT ANTIGUO - COMENTADO
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
            // Log para Welcome específicamente
            if matches!(msg, ServerMessage::Welcome { .. }) {
                println!(
                    "📨 [Server->Socket] Enviando Welcome a {} por socket...",
                    addr
                );
            }

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

                // Log específico para Welcome
                if matches!(msg, ServerMessage::Welcome { .. }) {
                    println!(
                        "✅ [Server->Socket] Welcome enviado a {} por socket ({} bytes)",
                        addr,
                        data.len()
                    );
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

                                // Obtener configuración y mapa del estado
                                let (config, map) = {
                                    let s = state.lock().unwrap();
                                    (s.game_config.clone(), s.map.clone())
                                };

                                println!(
                                    "📤 [Server] Enviando Welcome a jugador {} por canal...",
                                    id
                                );
                                let send_result = tx
                                    .send(ServerMessage::Welcome {
                                        player_id: id,
                                        game_config: config,
                                        map,
                                    })
                                    .await;

                                if send_result.is_err() {
                                    println!("❌ [Server] Error al enviar Welcome por canal para jugador {}", id);
                                } else {
                                    println!(
                                        "✅ [Server] Welcome enviado por canal para jugador {}",
                                        id
                                    );
                                }

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

    // Notificar desconexión al sistema de juego
    if let Some(id) = player_id {
        let _ = event_tx.send(NetworkEvent::PlayerDisconnected { id }).await;
        println!("🚪 Jugador {} desconectado", id);
    }
}

*/
// Fin del comentario de handle_client

fn update_input_manager(mut game_input: ResMut<GameInputManager>) {
    game_input.tick();
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
        // Pelota: colisiona con todo EXCEPTO líneas solo-jugadores (GROUP_6)
        CollisionGroups::new(Group::GROUP_3, Group::ALL ^ Group::GROUP_6),
        SolverGroups::new(Group::GROUP_3, Group::ALL ^ Group::GROUP_6),
        AdditionalMassProperties::Mass(config.ball_mass),
        Friction {
            coefficient: config.ball_friction,
            combine_rule: CoefficientCombineRule::Average,
        },
        Restitution {
            coefficient: config.ball_restitution,
            combine_rule: CoefficientCombineRule::Min,
        },
        Damping {
            linear_damping: config.ball_linear_damping,
            angular_damping: config.ball_angular_damping,
        },
        ExternalImpulse::default(),
        ExternalForce::default(),
    ));

    // Intentar cargar mapa
    let map_loaded = if let Some(map_path) = &config.map_path {
        match map::load_map(map_path) {
            Ok(haxball_map) => {
                let converter = map::MapConverter::new();
                converter.spawn_map_geometry(&mut commands, &haxball_map, config.wall_restitution);
                true
            }
            Err(e) => {
                eprintln!("⚠️  Failed to load map: {}", e);
                eprintln!("   Falling back to default walls");
                false
            }
        }
    } else {
        false
    };

    // Fallback: crear paredes por defecto si no hay mapa o falló la carga
    if !map_loaded || config.use_default_walls {
        spawn_default_walls(&mut commands, &config);
    }

    println!("✅ Juego configurado");
}

// Función auxiliar: spawner paredes por defecto
fn spawn_default_walls(commands: &mut Commands, config: &GameConfig) {
    let wall_thickness = 10.0;
    let wall_collision = CollisionGroups::new(Group::GROUP_1, Group::ALL);

    // Pared superior
    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(config.arena_width / 2.0, wall_thickness),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(0.0, config.arena_height / 2.0, 0.0),
        GlobalTransform::default(),
    ));

    // Pared inferior
    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(config.arena_width / 2.0, wall_thickness),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(0.0, -config.arena_height / 2.0, 0.0),
        GlobalTransform::default(),
    ));

    // Pared izquierda
    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(wall_thickness, config.arena_height / 2.0),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(-config.arena_width / 2.0, 0.0, 0.0),
        GlobalTransform::default(),
    ));

    // Pared derecha
    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(wall_thickness, config.arena_height / 2.0),
        wall_collision,
        Restitution::coefficient(config.wall_restitution),
        Transform::from_xyz(config.arena_width / 2.0, 0.0, 0.0),
        GlobalTransform::default(),
    ));

    println!("✅ Default walls spawned");
}

fn process_network_messages(
    mut commands: Commands,
    mut network_rx: ResMut<NetworkReceiver>,
    network_tx: Res<NetworkSender>,
    config: Res<GameConfig>,
    loaded_map: Res<LoadedMap>,
    mut game_input: ResMut<GameInputManager>,
    mut players: Query<(&mut Player, Entity)>,
) {
    while let Ok(event) = network_rx.0.lock().unwrap().try_recv() {
        match event {
            NetworkEvent::NewPlayer { id, name, peer_id } => {
                // Agregar jugador al GameInputManager
                game_input.add_player(id);

                // Enviar WELCOME al nuevo jugador
                let welcome_msg = ControlMessage::Welcome {
                    player_id: id,
                    game_config: config.clone(),
                    map: loaded_map.0.clone(),
                };

                if let Ok(data) = bincode::serialize(&welcome_msg) {
                    println!("📤 Enviando WELCOME a jugador {}", id);
                    let _ = network_tx.0.send(OutgoingMessage::ToOne {
                        peer_id,
                        channel: 0, // Canal reliable
                        data,
                    });
                }

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

                // Spawn lógica del jugador (Player) - Usando peer_id ahora
                commands.spawn(Player {
                    sphere: sphere_entity,
                    id,
                    name: name.clone(),
                    kick_charge: 0.0,
                    kick_charging: false,
                    curve_charge: 0.0,
                    curve_charging: false,
                    peer_id, // Guardamos peer_id para enviar mensajes
                    is_ready: false,
                    not_interacting: false,
                    is_sliding: false,
                    slide_direction: Vec2::ZERO,
                    slide_timer: 0.0,
                    ball_target_position: None,
                    stamin: 1.0,
                });

                println!("✅ Jugador {} spawneado: {}", id, name);
            }

            NetworkEvent::PlayerInput { peer_id, input } => {
                // Buscar el player_id real usando el peer_id
                for (player, _) in players.iter() {
                    if player.peer_id == peer_id {
                        game_input.update_input(player.id, input);
                        break;
                    }
                }
            }

            NetworkEvent::PlayerDisconnected { peer_id } => {
                for (player, entity) in players.iter() {
                    if player.peer_id == peer_id {
                        // Despawnear tanto Player como Sphere (igual que RustBall)
                        commands.entity(player.sphere).despawn();
                        commands.entity(entity).despawn();
                        println!("❌ Jugador {} removido", player.id);
                        break;
                    }
                }
            }

            NetworkEvent::PlayerReady { peer_id } => {
                for (mut player, _) in players.iter_mut() {
                    if player.peer_id == peer_id {
                        player.is_ready = true;
                        println!(
                            "✅ Jugador {} marcado como READY en el loop de juego",
                            player.id
                        );
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
fn handle_collision_player(
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

// Sistema de carga de patada - ahora funciona con S, A o D
fn charge_kick(
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

                    /*let should_release = !any_kick_button && player.kick_charging;
                    if should_release {
                        player.kick_charging = false;
                    }*/
                }
            }
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

fn look_at_ball(
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
fn attract_ball(
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
fn detect_slide(
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
fn execute_slide(
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

fn dash_first_touch_ball(
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
    config: Res<GameConfig>,
    mut broadcast_timer: ResMut<BroadcastTimer>,
    mut tick: ResMut<GameTick>,
    players: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), With<Sphere>>,
    ball: Query<(&Transform, &Velocity, &Ball), Without<Sphere>>,
    network_tx: Res<NetworkSender>,
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
                    is_sliding: player.is_sliding,
                    not_interacting: player.not_interacting,
                    ball_target_position: player.ball_target_position,
                    stamin_charge: player.stamin,
                })
            } else {
                println!(
                    "⚠️  No se pudo obtener Transform/Velocity para jugador {}",
                    player.id
                );
                None
            }
        })
        .collect();

    // Log cada 60 ticks (2 segundos)
    if tick.0 % 60 == 0 {
        println!(
            "📊 [Tick {}] Jugadores: {}, Ready: {}",
            tick.0,
            players.iter().count(),
            players.iter().filter(|p| p.is_ready).count()
        );
    }

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

    // Extraer datos de game_state (ServerMessage) para GameDataMessage
    let (tick_num, timestamp_num, players_vec, ball_state_data) = match game_state {
        ServerMessage::GameState {
            tick,
            timestamp,
            players,
            ball,
        } => (tick, timestamp, players, ball),
        _ => return, // No debería pasar
    };

    // Crear GameDataMessage para el canal unreliable
    let game_data_msg = GameDataMessage::GameState {
        tick: tick_num,
        timestamp: timestamp_num,
        players: players_vec,
        ball: ball_state_data,
    };

    // Serializar y broadcast a todos los jugadores READY
    if let Ok(data) = bincode::serialize(&game_data_msg) {
        // Enviar solo a jugadores que están ready
        for player in players.iter() {
            if player.is_ready {
                let _ = network_tx.0.send(OutgoingMessage::ToOne {
                    peer_id: player.peer_id,
                    channel: 1, // Canal unreliable para GameState
                    data: data.clone(),
                });
            }
        }
    }
}

fn recover_stamin(
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
