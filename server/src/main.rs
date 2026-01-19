use bevy::math::VectorSpace;
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use clap::Parser;
use matchbox_socket::{PeerId, PeerState, WebRtcSocket};
use shared::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

mod engine;
mod input;
mod map;
mod network;

use engine::*;
use input::{GameAction, InputSource, NetworkInputSource};
use network::*;

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
        .add_systems(Startup, (configure_rapier, setup_game).chain())
        .add_systems(
            FixedUpdate,
            (
                update_input_manager,
                process_network_messages,
                look_at_ball,
                detect_slide,
                execute_slide,
                move_players,
                handle_collision_player,
                charge_kick,
                kick_ball,
                apply_magnus_effect,
                attract_ball,
                push_ball_on_contact,
                auto_touch_ball_while_running,
                dash_first_touch_ball,
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
pub struct NetworkReceiver(pub Arc<Mutex<mpsc::Receiver<NetworkEvent>>>);

#[derive(Resource)]
pub struct NetworkSender(pub mpsc::Sender<OutgoingMessage>);

#[derive(Resource)]
pub struct GameTick(pub u32);

#[derive(Resource)]
pub struct BroadcastTimer(pub Timer);

#[derive(Resource)]
pub struct LoadedMap(pub Option<shared::map::Map>);

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
pub struct Player {
    pub sphere: Entity,     // Referencia a la entidad física (igual que RustBall)
    pub slide_cube: Entity, // Referencia al cubo de dirección/slide
    pub id: u32,
    pub name: String,
    pub kick_charge: f32,
    pub kick_charging: bool,
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
    pub active_movement: Option<shared::protocol::PlayerMovement>,

    // Team
    pub team_index: u8,
}

// Marker component para la entidad física del jugador (igual que RustBall)
#[derive(Component)]
pub struct Sphere;

// Marker component para el cubo de dirección/slide
#[derive(Component)]
pub struct SlideCube {
    pub owner_id: u32,
}

#[derive(Component)]
pub struct Ball {
    pub angular_velocity: f32,
}

// ============================================================================
// NETWORK STATE
// ============================================================================

pub struct NetworkState {
    pub next_player_id: u32,
    pub game_config: GameConfig,
    pub map: Option<shared::map::Map>,
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
    PlayerDisconnected {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
    },
    PlayerReady {
        peer_id: PeerId, // Buscar por peer_id en lugar de por id
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

fn configure_rapier(mut rapier_config: Query<&mut RapierConfiguration>) {
    if let Ok(mut config) = rapier_config.single_mut() {
        config.gravity = Vec2::ZERO;
    }
}

fn setup_game(mut commands: Commands, config: Res<GameConfig>) {
    println!("⚽ Configurando juego...");

    // Crear pelota
    commands.spawn((
        Ball {
            angular_velocity: 0.0,
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        GlobalTransform::default(),
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
        Ccd::enabled(),
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
