use crate::shared::*;
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use matchbox_socket::PeerId;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::engine::*;
use super::input::{GameAction, InputSource, NetworkInputSource};
use super::match_logic::*;
use super::network::*;

/// Resource for managing player slots in the match
#[derive(Resource, Default)]
pub struct HostMatchSlots(pub MatchSlots);

/// Resource que encapsula el estado del partido activo en el host
#[derive(Resource, Default)]
pub struct HostMatchGame(pub crate::shared::match_game::MatchGame);

/// Señal compartida: `true` mientras la pelota está bloqueada en el punto de
/// reanudación de una jugada a balón parado. Los sistemas de engine.rs la leen
/// para no empujar la pelota con contacto suave.
#[derive(Resource, Default)]
pub struct SetPieceLock(pub bool);

pub fn host(
    map: Option<String>,
    default_map_content: &'static str,
    scale: f32,
    room: String,
    server_host: String,
    room_name: String,
    max_players: u8,
) {
    println!("🎮 Haxball Host - Iniciando...");

    // Clone map path for later use in proxy registration
    let map_name_for_proxy = map.clone();

    // Configurar GameConfig con el mapa
    let (game_config, loaded_map) = if let Some(map_path) = map {
        // Cargar mapa externo
        println!("🗺️  Cargando mapa: {}", map_path);

        let loaded_map = match super::map::load_map(&map_path) {
            Ok(mut m) => {
                println!("   ✅ Mapa cargado: {}", m.name);

                if (scale - 1.0).abs() > 0.01 {
                    println!("   📏 Aplicando escala: {}x", scale);
                    m.scale(scale);
                }

                Some(m)
            }
            Err(e) => {
                eprintln!("   ⚠️  Error cargando mapa: {}", e);
                eprintln!("   Continuando con mapa embebido por defecto");
                None
            }
        };

        let config = GameConfig {
            map_path: Some(map_path),
            ..Default::default()
        };

        (config, loaded_map)
    } else {
        // Usar mapa embebido por defecto
        println!("🏟️  Usando mapa embebido por defecto");

        let loaded_map =
            match super::map::load_map_from_str(default_map_content, "cancha_grande (embebido)") {
                Ok(mut m) => {
                    if (scale - 1.0).abs() > 0.01 {
                        println!("   📏 Aplicando escala: {}x", scale);
                        m.scale(scale);
                    }
                    Some(m)
                }
                Err(e) => {
                    eprintln!("   ⚠️  Error cargando mapa embebido: {}", e);
                    None
                }
            };

        let config = GameConfig {
            map_path: None,
            ..Default::default()
        };

        (config, loaded_map)
    };

    println!("🌐 Conectando al proxy en: {}", server_host);

    let (network_tx, network_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel();

    // Clonar loaded_map para usarlo en ambos lugares
    // La versión mínima del cliente es la versión actual del protocolo
    let min_version = protocol::ProtocolVersion::current();
    println!("📋 Versión del servidor: {}", min_version);
    let network_state = Arc::new(Mutex::new(NetworkState {
        next_player_id: 1,
        min_client_version: min_version,
    }));

    // Iniciar servidor WebRTC (se conecta al proxy)
    let room = room.clone();
    let server_host = server_host.clone();
    let room_name = room_name.clone();
    let max_players = max_players;
    let map_name = map_name_for_proxy;
    std::thread::spawn(move || {
        start_webrtc_server(
            network_tx,
            network_state,
            room,
            outgoing_rx,
            server_host,
            room_name,
            max_players,
            map_name,
        );
    });

    // Initialize MatchSlots - first player (player_id 1) will be admin
    let mut initial_slots = MatchSlots::default();
    initial_slots.add_admin(1); // First player to join is admin

    App::new()
        .add_plugins(
            MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            )),
        )
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .insert_resource(game_config)
        .insert_resource(NetworkReceiver(Arc::new(Mutex::new(network_rx))))
        .insert_resource(NetworkSender(outgoing_tx))
        .insert_resource(LoadedMap(loaded_map.clone()))
        .insert_resource(GameTick(0))
        .insert_resource(BroadcastTimer(Timer::from_seconds(
            1.0 / 60.0,
            TimerMode::Repeating,
        ))) // 60 Hz
        .insert_resource(HostMatchSlots(initial_slots))
        .insert_resource(HostMatchGame::default())
        .insert_resource(SetPieceLock::default())
        .init_resource::<GameInputManager>()
        .add_systems(Startup, (configure_rapier, setup_game, setup_map).chain())
        .add_systems(
            FixedUpdate,
            (
                // Grupo 1: física y red (sistemas originales)
                (
                    update_input_manager,
                    process_network_messages,
                    look_at_ball,
                    toggle_mode,
                    detect_slide,
                    execute_slide,
                    move_players,
                    handle_collision_player,
                    charge_kick,
                    prepare_kick_ball,
                    detect_contact_and_kick,
                    apply_magnus_effect,
                    attract_ball,
                    update_kick_memory_timer,
                    auto_touch_ball_while_running,
                    dash_first_touch_ball,
                    broadcast_game_state,
                    recover_stamin,
                )
                    .chain(),
                // Grupo 2: lógica de partido (se ejecuta después del grupo 1)
                (
                    update_ball_touch, // registra último toque y limpia pending_set_piece
                    tick_match_time,
                    reset_ball_for_kickoff, // antes de detect_goal: limpia pending_kickoff
                    detect_goal,
                    detect_ball_out,             // detecta salida y arranca timer
                    apply_set_piece_position,    // teleporta pelota al expirar el timer
                    enforce_set_piece_exclusion, // empuja al equipo contrario fuera del radio
                    update_set_piece_lock,       // actualiza SetPieceLock y Dominance de la pelota
                    broadcast_match_state,
                )
                    .chain(),
            )
                .chain(),
        )
        .run();
}

// ============================================================================
// RECURSOS Y COMPONENTES
// ============================================================================

#[derive(Resource)]
pub struct NetworkReceiver(pub Arc<Mutex<mpsc::Receiver<NetworkEvent>>>);

#[derive(Resource)]
pub struct NetworkSender(pub mpsc::Sender<OutgoingMessage>);

#[derive(Resource)]
pub struct GameTick(pub u32);

#[derive(Resource)]
pub struct BroadcastTimer(pub Timer);

#[derive(Resource)]
pub struct LoadedMap(pub Option<crate::shared::map::Map>);

/// GameInputManager usando NetworkInputSource
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

    pub fn remove_player(&mut self, player_id: u32) {
        self.sources.remove(&player_id);
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

    pub fn get_curve_dir(&self, player_id: u32) -> Vec2 {
        self.sources
            .get(&player_id)
            .map(|s| s.get_curve_dir())
            .unwrap_or(Vec2::ZERO)
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

// Estructura del Player referencia a Sphere
#[derive(Component)]
pub struct Player {
    pub sphere: Entity,     // Referencia a la entidad física
    pub slide_cube: Entity, // Referencia al cubo de dirección/slide
    pub id: u32,
    pub name: String,
    /// Vector de carga acumulado.
    /// Disparo por defecto de X (sin comba): kick_vec.x = potencia fija, kick_vec.y = 0.
    /// Carga con comba (WASD): kick_vec crece en dirección nseo_vec; length() = potencia.
    pub kick_vec: Vec2,
    pub is_straight_kick: bool, // true si es el disparo por defecto de X (sin comba/curva)
    pub kick_charging: bool,
    /// true si la carga es el disparo por defecto de X (sin carga previa):
    /// se dispara solo en el próximo contacto, sin esperar otra pulsación de X.
    pub kick_instant: bool,
    pub kick_memory_timer: f32, // Timer de 1 segundo para potencia memorizada
    pub peer_id: PeerId, // Matchbox peer ID para enviar mensajes
    pub is_ready: bool,

    pub not_interacting: bool,
    // Barrida/Slide
    pub is_sliding: bool,
    pub slide_direction: Vec2,
    pub slide_timer: f32,

    pub ball_target_position: Option<Vec2>,
    pub stamin: f32,

    // Slide cube state (para física del servidor)
    pub slide_cube_active: bool,
    pub slide_cube_offset: Vec2,
    pub slide_cube_scale: f32,

    // Movimiento visual activo
    pub active_movement: Option<crate::shared::protocol::PlayerMovement>,

    // Team
    pub team_index: u8,

    // Mode (cubo grande, esfera chica)
    pub mode_cube_active: bool,
}

// Marker component para la entidad física del jugador
#[derive(Component)]
pub struct Sphere;

// Marker component para el cubo de dirección/slide
#[derive(Component)]
pub struct SlideCube {}

#[derive(Component)]
pub struct Ball {
    pub angular_velocity: f32,
}

// ============================================================================
// NETWORK STATE
// ============================================================================

pub struct NetworkState {
    pub next_player_id: u32,
    /// Versión mínima del cliente requerida por este servidor
    pub min_client_version: protocol::ProtocolVersion,
}

pub enum NetworkEvent {
    NewPlayer {
        id: u32,
        name: String,
        peer_id: PeerId, // Matchbox peer ID
    },
    PlayerInput {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
        input: PlayerInput,
    },
    /// Input identificado por player_id directamente (para multijugador local)
    PlayerInputById { player_id: u32, input: PlayerInput },
    PlayerDisconnected {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
    },
    PlayerReady {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
    },
    /// El jugador solicitó salir voluntariamente
    PlayerLeave { player_id: u32 },
    /// Admin moves a player to a different slot
    MovePlayer {
        admin_peer_id: PeerId,
        player_id: u32,
        team_index: Option<u8>,
        is_starter: Option<bool>,
    },
    /// Admin kicks a player from the room
    KickPlayer {
        admin_peer_id: PeerId,
        player_id: u32,
    },
    /// Admin toggles admin status for another player
    ToggleAdmin {
        admin_peer_id: PeerId,
        player_id: u32,
        is_admin: bool,
    },
    /// Admin solicita iniciar el partido
    StartMatch {
        admin_peer_id: PeerId,
        duration_secs: f32,
    },
}

/// Mensajes salientes del servidor a los clientes
pub enum OutgoingMessage {
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
// GAME SETUP
// ============================================================================

fn configure_rapier(
    mut rapier_config: Query<&mut RapierConfiguration>,
    mut rapier_context: Query<&mut RapierContextSimulation>,
) {
    if let Ok(mut config) = rapier_config.single_mut() {
        config.gravity = Vec2::ZERO;
    }

    if let Ok(mut context) = rapier_context.single_mut() {
        // Por defecto Rapier permite corregir solapamientos a hasta
        // normalized_max_corrective_velocity * length_unit = 10.0 * 100.0 = 1000 u/s.
        // Con la pelota tan liviana (ball_mass=0.1) frente al jugador, si el jugador
        // queda con velocidad congelada en 0 mientras solapa la pelota (típico al
        // acercarse "a tironcitos", soltando la tecla justo al tocar), toda esa
        // corrección se descarga de una sola vez sobre la pelota, disparándola.
        // Bajamos el tope bien por debajo de player_speed_walking (300) para que
        // el solapamiento se resuelva gradualmente en varios ticks en vez de un
        // salto explosivo.
        context.integration_parameters.normalized_max_corrective_velocity = 1.5;
    }
}

fn setup_game(mut commands: Commands, config: Res<GameConfig>) {
    println!("⚽ Configurando juego...");

    // Crear pelota (dividido en dos tuplas para no superar el límite de 15 de Bevy)
    commands.spawn((
        (
            Ball {
                angular_velocity: 0.0,
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
            GlobalTransform::default(),
            RigidBody::Dynamic,
            Collider::ball(config.ball_radius),
            Velocity::zero(),
            // Colisiona con todo EXCEPTO líneas solo-jugadores (GROUP_6)
            CollisionGroups::new(Group::GROUP_3, Group::ALL ^ Group::GROUP_6),
            SolverGroups::new(Group::GROUP_3, Group::ALL ^ Group::GROUP_6),
        ),
        (
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
            Ccd::enabled(),
            Dominance::group(0), // Se sube a 127 durante jugadas a balón parado
        ),
    ));

    println!("✅ Juego configurado");
}
