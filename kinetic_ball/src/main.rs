use bevy::asset::uuid_handle;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{visibility::RenderLayers, RenderTarget, ScalingMode};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureDimension, TextureFormat, TextureUsages};
use bevy::shader::{Shader, ShaderRef};
use bevy::sprite::Anchor;
use bevy::sprite_render::{Material2d, Material2dPlugin};
use bevy::ui::widget::ViewportNode;
use bevy::ui::UiTargetCamera;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_rapier2d::prelude::*;
use clap::Parser;
use matchbox_socket::WebRtcSocket;
use shared::movements::{get_movement, AnimatedProperty};
use shared::protocol::PlayerMovement;
use shared::protocol::{
    ControlMessage, GameConfig, GameDataMessage, PlayerInput, ProtocolVersion, ServerMessage,
};
use std::f32::consts::FRAC_PI_2;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use bevy::math::VectorSpace;

mod keybindings;
use keybindings::{
    key_code_display_name, load_app_config, load_gamepad_bindings_map, load_keybindings,
    save_gamepad_bindings_map, save_keybindings, AppConfig, DetectedGamepadEvent, GameAction,
    GamepadBindingsConfig, GamepadBindingsMap, GamepadConfigUIState, GilrsWrapper,
    KeyBindingsConfig, RawGamepadInput, SettingsUIState,
};

mod local_players;
use local_players::{
    detect_gamepads, idx_to_gilrs_axis, read_local_player_input, AvailableInputDevices,
    InputDevice, LocalPlayer, LocalPlayers, LocalPlayersUIState,
};

mod host;
mod shared;

// ============================================================================
// ASSETS EMBEBIDOS EN EL BINARIO
// ============================================================================

const BALL_PNG: &[u8] = include_bytes!("../assets/ball.png");
const DEFAULT_MAP: &str = include_str!("../assets/cancha_grande.hbs");
const SPLIT_SCREEN_SHADER_SRC: &str =
    include_str!("../assets/shaders/split_screen_compositor.wgsl");

// Handle constante para el shader de split-screen (usando UUID fijo)
const SPLIT_SCREEN_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("1a2b3c4d-5e6f-7890-abcd-ef1234567890");

// ============================================================================
// RECURSO PARA MOVIMIENTOS ACTIVOS
// ============================================================================

#[derive(Resource, Default)]
struct GameTick(u32);

#[derive(Parser, Debug, Clone)]
#[command(name = "Haxball Client")]
#[command(about = "Cliente del juego Haxball", long_about = None)]
struct Args {
    /// Host del proxy (sin protocolo). Ejemplo: localhost:3537 o proxy.ejemplo.com
    #[arg(short, long)]
    server: Option<String>,

    /// Nombre de la sala/room
    #[arg(short, long, default_value = "game_server")]
    room: String,

    /// Nombre del jugador
    #[arg(long, default_value = "Player")]
    name: String,
}

// ============================================================================
// ESTADOS DE LA APLICACIÓN
// ============================================================================

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
enum AppState {
    #[default]
    Menu,
    Settings,
    LocalPlayersSetup,
    GamepadConfig,
    RoomSelection,
    CreateRoom,
    HostingRoom,
    Connecting,
    InGame,
}

// ============================================================================
// ROOM INFO (from proxy API)
// ============================================================================

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum RoomStatus {
    Open,
    Full,
    Closed,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RoomInfo {
    room_id: String,
    name: String,
    max_players: u8,
    current_players: u8,
    map_name: Option<String>,
    status: RoomStatus,
    #[serde(default)]
    min_version: Option<String>,
}

#[derive(Resource, Default)]
struct RoomList {
    rooms: Vec<RoomInfo>,
    loading: bool,
    error: Option<String>,
}

#[derive(Resource, Default)]
struct RoomFetchChannel {
    receiver: Option<Arc<Mutex<mpsc::Receiver<Result<Vec<RoomInfo>, String>>>>>,
}

#[derive(Resource, Default)]
struct SelectedRoom {
    room_id: Option<String>,
}

#[derive(Resource)]
struct CreateRoomConfig {
    room_name: String,
    max_players: u8,
    map_path: String,
    scale: f32,
}

impl Default for CreateRoomConfig {
    fn default() -> Self {
        Self {
            room_name: String::from("mi_sala"),
            max_players: 4,
            map_path: String::new(), // Vacío = usar mapa embebido por defecto
            scale: 1.0,
        }
    }
}

// ============================================================================
// CONFIGURACIÓN DE CONEXIÓN (valores editables en el menú)
// ============================================================================

#[derive(Resource)]
struct ConnectionConfig {
    server_host: String, // Host sin protocolo: localhost:3536 o api.example.com
    room: String,
    player_name: String,
}

impl ConnectionConfig {
    fn from_args(args: &Args, app_config: &AppConfig) -> Self {
        Self {
            server_host: args
                .server
                .clone()
                .unwrap_or_else(|| app_config.server.clone()),
            room: args.room.clone(),
            player_name: args.name.clone(),
        }
    }

    /// Determina si debe usar conexión segura (HTTPS/WSS)
    fn is_secure(&self) -> bool {
        let host = &self.server_host;
        // Usar HTTP/WS solo para desarrollo local
        // Todo lo demás usa HTTPS/WSS
        !host.starts_with("localhost") && !host.starts_with("127.0.0.1")
    }

    /// URL HTTP/HTTPS para llamadas REST API
    fn http_url(&self) -> String {
        let protocol = if self.is_secure() { "https" } else { "http" };
        format!("{}://{}", protocol, self.server_host)
    }

    /// URL WebSocket WS/WSS para conexiones WS
    fn ws_url(&self) -> String {
        let protocol = if self.is_secure() { "wss" } else { "ws" };
        format!("{}://{}", protocol, self.server_host)
    }
}

// ============================================================================
// FUNCIONES HELPER DE COLORES
// ============================================================================

/// Convierte RGB a HSV
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max == 0.0 { 0.0 } else { delta / max };

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };

    (h, s, v)
}

/// Convierte HSV a RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (v, v, v);
    }

    let h = h * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Calcula el color complementario rotando el Hue 180 grados en HSV
fn complementary_color(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let h_opposite = (h + 0.5).rem_euclid(1.0);
    hsv_to_rgb(h_opposite, s, v)
}

/// Genera un color único para un jugador, evitando la zona verde del fondo del minimapa.
/// Usa golden ratio para distribución uniforme y evita hues 80°-160° (zona verde).
fn generate_unique_player_color(player_colors: &mut PlayerColors) -> Color {
    const GOLDEN_RATIO: f32 = 0.618033988749895;

    // Calcular el hue base usando golden ratio para máxima separación
    let raw_hue = player_colors.next_hue_offset;
    player_colors.next_hue_offset = (player_colors.next_hue_offset + GOLDEN_RATIO) % 1.0;

    // Evitar zona verde (80°-160° = 0.222-0.444 en rango 0-1)
    // Mapear el hue a los rangos válidos: 0°-80° (0.0-0.222) y 160°-360° (0.444-1.0)
    // Rango válido total: 0.222 + 0.556 = 0.778 del espectro
    let valid_range = 0.778;
    let scaled_hue = raw_hue * valid_range;

    let final_hue = if scaled_hue < 0.222 {
        // Zona roja/naranja/amarilla (0° - 80°)
        scaled_hue
    } else {
        // Zona azul/magenta/rosa (160° - 360°)
        scaled_hue + 0.222 // Saltar la zona verde
    };

    // Saturación alta (0.85) y Value alto (0.95) para visibilidad
    let (r, g, b) = hsv_to_rgb(final_hue, 0.85, 0.95);
    Color::srgb(r, g, b)
}

/// Calcula el color del jugador y su color opuesto para barras/texto
/// basándose en el índice de equipo y los colores definidos en la configuración
fn get_team_colors(team_index: u8, team_colors: &[(f32, f32, f32)]) -> (Color, Color) {
    let team_color = team_colors
        .get(team_index as usize)
        .copied()
        .unwrap_or((0.5, 0.5, 0.5));

    let player_color = Color::srgb(team_color.0, team_color.1, team_color.2);
    let (r, g, b) = complementary_color(team_color.0, team_color.1, team_color.2);
    let opposite_color = Color::srgb(r, g, b);

    (player_color, opposite_color)
}

fn main() {
    // Inicializar el CryptoProvider de rustls antes de cualquier uso de TLS
    // Necesario en Mac donde no se puede autodetectar
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = Args::parse();
    let app_config = load_app_config();
    println!("🎮 Haxball Client - Iniciando...");

    // Bevy
    println!("🎨 [Bevy] Intentando abrir ventana...");
    let mut app = App::new();

    // Insertar gilrs wrapper solo si se inicializa correctamente
    if let Some(gilrs) = GilrsWrapper::new() {
        app.insert_resource(gilrs);
    }

    app.insert_resource(bevy::winit::WinitSettings::game())
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.15)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "🐐⚽ kinetic-ball ⚽🐐".to_string(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .add_plugins(Material2dPlugin::<SplitScreenMaterial>::default())
        // Estado de la aplicación
        .init_state::<AppState>()
        // Configuración de conexión (valores iniciales desde args y config)
        .insert_resource(ConnectionConfig::from_args(&args, &app_config))
        // Recursos del juego (se inicializan vacíos, se llenan al conectar)
        .insert_resource(GameConfig::default())
        .insert_resource(NetworkChannels::default())
        .insert_resource(MyPlayerId(None))
        .insert_resource(LoadedMap::default())
        .insert_resource(PreviousInput::default())
        .insert_resource(GameTick::default())
        // Keybindings y Gamepad Bindings
        .insert_resource(load_keybindings())
        .insert_resource(load_gamepad_bindings_map())
        .insert_resource(SettingsUIState::default())
        .insert_resource(GamepadConfigUIState::default())
        .insert_resource(DetectedGamepadEvent::default())
        // Room selection resources
        .insert_resource(RoomList::default())
        .insert_resource(RoomFetchChannel::default())
        .insert_resource(SelectedRoom::default())
        // Create room resources
        .insert_resource(CreateRoomConfig::default())
        // Local players resources
        .insert_resource(LocalPlayers::new(4)) // Máximo 4 jugadores locales
        .insert_resource(AvailableInputDevices::default())
        .insert_resource(LocalPlayersUIState::default())
        // Player colors for split-screen
        .insert_resource(PlayerColors::default())
        // Dynamic split-screen resources
        .insert_resource(DynamicSplitState::default())
        .insert_resource(SplitScreenTextures::default())
        // Cargar assets embebidos al inicio (antes de todo)
        .add_systems(Startup, load_embedded_assets)
        // Sistemas de input y detección
        .add_systems(Update, (detect_gamepads, gilrs_event_system).chain())
        // Sistemas de menú (solo en estado Menu)
        .add_systems(OnEnter(AppState::Menu), setup_menu_camera_if_needed)
        .add_systems(
            EguiPrimaryContextPass,
            menu_ui.run_if(in_state(AppState::Menu)),
        )
        // Sistemas de configuración (solo en estado Settings)
        .add_systems(OnEnter(AppState::Settings), setup_menu_camera_if_needed)
        .add_systems(
            EguiPrimaryContextPass,
            settings_ui.run_if(in_state(AppState::Settings)),
        )
        // Sistemas de configuración de jugadores locales (solo en estado LocalPlayersSetup)
        .add_systems(
            OnEnter(AppState::LocalPlayersSetup),
            setup_menu_camera_if_needed,
        )
        .add_systems(
            EguiPrimaryContextPass,
            local_players_setup_ui.run_if(in_state(AppState::LocalPlayersSetup)),
        )
        // Sistemas de configuración de gamepad (solo en estado GamepadConfig)
        .add_systems(
            OnEnter(AppState::GamepadConfig),
            setup_menu_camera_if_needed,
        )
        .add_systems(
            EguiPrimaryContextPass,
            gamepad_config_ui.run_if(in_state(AppState::GamepadConfig)),
        )
        // Sistemas de selección de sala (solo en estado RoomSelection)
        .add_systems(
            OnEnter(AppState::RoomSelection),
            (setup_menu_camera_if_needed, fetch_rooms),
        )
        .add_systems(
            EguiPrimaryContextPass,
            room_selection_ui.run_if(in_state(AppState::RoomSelection)),
        )
        .add_systems(
            Update,
            check_rooms_fetch.run_if(in_state(AppState::RoomSelection)),
        )
        // Sistemas de crear sala (solo en estado CreateRoom)
        .add_systems(OnEnter(AppState::CreateRoom), setup_menu_camera_if_needed)
        .add_systems(
            EguiPrimaryContextPass,
            create_room_ui.run_if(in_state(AppState::CreateRoom)),
        )
        // Sistemas de hosting (solo en estado HostingRoom)
        .add_systems(
            OnEnter(AppState::HostingRoom),
            (setup_menu_camera_if_needed, start_hosting),
        )
        .add_systems(
            EguiPrimaryContextPass,
            hosting_ui.run_if(in_state(AppState::HostingRoom)),
        )
        // Sistema de conexión (solo en estado Connecting)
        .add_systems(
            OnEnter(AppState::Connecting),
            (cleanup_menu_camera, start_connection).chain(),
        )
        .add_systems(
            Update,
            check_connection.run_if(in_state(AppState::Connecting)),
        )
        // Setup del juego (solo al entrar a InGame)
        .add_systems(OnEnter(AppState::InGame), setup)
        // Lógica de red y entrada (frecuencia fija, solo en InGame)
        .add_systems(
            FixedUpdate,
            (handle_multi_player_input, process_network_messages)
                .run_if(in_state(AppState::InGame)),
        )
        // Lógica visual y renderizado (solo en InGame)
        .add_systems(
            Update,
            (
                adjust_field_for_map,
                interpolate_entities,
                keep_name_horizontal,
                update_split_screen_state,
                camera_follow_player_and_ball,
                camera_zoom_control,
                update_camera_viewports,
                update_split_compositor,
                update_charge_bar,
                update_player_sprite,
                process_movements,
                update_mode_visuals,
                update_dash_cooldown,
                spawn_minimap_dots,
                sync_minimap_dots,
                sync_minimap_names,
                cleanup_minimap_dots,
                animate_keys,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .run();

    println!("✅ [Bevy] App::run() ha finalizado normalmente");
}

/// Sistema que procesa los eventos de gilrs para mantener el estado actualizado
/// y capturar inputs durante la configuración de gamepad
fn gilrs_event_system(
    gilrs: Option<ResMut<GilrsWrapper>>,
    gamepad_ui_state: Res<GamepadConfigUIState>,
    mut detected_event: ResMut<DetectedGamepadEvent>,
) {
    // NO limpiar el evento aquí - la UI lo limpiará cuando lo consuma
    // Esto permite que el evento persista entre frames/schedules

    if let Some(gilrs) = gilrs {
        if let Ok(mut gilrs_instance) = gilrs.gilrs.lock() {
            while let Some(gilrs::Event { id, event, .. }) = gilrs_instance.next_event() {
                // Si estamos en modo rebinding, capturar el input
                if gamepad_ui_state.is_rebinding() {
                    match event {
                        gilrs::EventType::ButtonPressed(button, code) => {
                            // Usar la función que maneja botones desconocidos
                            let idx = keybindings::gilrs_button_code_to_idx(button, code);
                            println!(
                                "🎮 [Gilrs] Botón detectado: {:?} code {:?} -> idx {}",
                                button, code, idx
                            );
                            detected_event.input = Some((id, RawGamepadInput::Button(idx)));
                        }
                        gilrs::EventType::AxisChanged(axis, value, code) => {
                            // Solo capturar si el valor es significativo
                            if value.abs() > 0.7 {
                                // Usar mapeo estándar o fallback al código
                                let idx =
                                    keybindings::gilrs_axis_to_idx(axis).unwrap_or_else(|| {
                                        // Fallback: usar el código raw
                                        let raw: u32 = code.into_u32();
                                        (raw & 0x07) as u8
                                    });
                                let input = if value > 0.0 {
                                    RawGamepadInput::AxisPositive(idx)
                                } else {
                                    RawGamepadInput::AxisNegative(idx)
                                };
                                println!(
                                    "🎮 [Gilrs] Eje detectado: {:?} code {:?} valor {} -> {:?}",
                                    axis, code, value, input
                                );
                                detected_event.input = Some((id, input));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

// ============================================================================
// RECURSOS
// ============================================================================

/// Assets embebidos cargados en memoria
#[derive(Resource, Default)]
struct EmbeddedAssets {
    ball_texture: Handle<Image>,
}

/// Canal de comunicación con el thread de red
/// El sender ahora envía (player_id, PlayerInput) para soportar múltiples jugadores locales
#[derive(Resource, Default)]
struct NetworkChannels {
    receiver: Option<Arc<Mutex<mpsc::Receiver<ServerMessage>>>>,
    sender: Option<mpsc::Sender<(u32, PlayerInput)>>,
}

#[derive(Resource)]
struct MyPlayerId(Option<u32>);

#[derive(Resource, Default)]
struct LoadedMap(Option<shared::map::Map>);

/// Colores únicos para cada jugador en el minimapa y nombres
#[derive(Resource, Default)]
struct PlayerColors {
    colors: std::collections::HashMap<u32, Color>, // server_player_id -> Color
    next_hue_offset: f32,
}

#[derive(Component)]
struct DefaultFieldLine;

#[derive(Component)]
struct MinimapFieldLine;

#[derive(Component)]
struct MapLineEntity; // Líneas del mapa cargado (reemplazo de Gizmos)

#[derive(Component)]
struct FieldBackground;

#[derive(Component)]
struct MenuCamera;

#[derive(Component)]
struct MinimapCamera;

#[derive(Component)]
struct PlayerDetailCamera {
    local_index: u8,
}

/// Cámara dedicada para UI que no tiene viewport (renderiza pantalla completa)
#[derive(Component)]
struct GameUiCamera;

/// Cámara que compone el split-screen final
#[derive(Component)]
struct CompositorCamera;

// ============================================================================
// DYNAMIC SPLIT-SCREEN SYSTEM
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SplitMode {
    #[default]
    Unified, // Una sola cámara siguiendo a ambos jugadores
    Transitioning, // Animando entre modos
    Split,         // Dos cámaras independientes
}

#[derive(Resource)]
struct DynamicSplitState {
    mode: SplitMode,
    split_factor: f32,    // 0.0 = unified, 1.0 = full split
    split_angle: f32,     // Ángulo de la línea divisoria en radianes
    merge_threshold: f32, // Distancia para fusionar (con histéresis)
    split_threshold: f32, // Distancia para separar
    /// Ratio del viewport visible (basado en zoom) usado para calcular umbrales
    viewport_visible_ratio: f32,
}

impl Default for DynamicSplitState {
    fn default() -> Self {
        Self {
            mode: SplitMode::Unified,
            split_factor: 0.0,
            split_angle: FRAC_PI_2, // Vertical por defecto
            merge_threshold: 600.0, // Distancia a la que se fusionan
            split_threshold: 800.0, // Distancia a la que se separan
            viewport_visible_ratio: 1.0,
        }
    }
}

/// Handles para las texturas de render target de cada cámara
#[derive(Resource, Default)]
struct SplitScreenTextures {
    camera1_texture: Option<Handle<Image>>,
    camera2_texture: Option<Handle<Image>>,
}

/// Material para el compositor de split-screen
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct SplitScreenMaterial {
    #[texture(0)]
    #[sampler(1)]
    camera1_texture: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    camera2_texture: Handle<Image>,
    /// x: angle, y: factor, z: center_x, w: center_y
    #[uniform(4)]
    split_params: Vec4,
}

impl Material2d for SplitScreenMaterial {
    fn fragment_shader() -> ShaderRef {
        SPLIT_SCREEN_SHADER_HANDLE.into()
    }
}

/// Componente para identificar el mesh que muestra el split-screen compuesto
#[derive(Component)]
struct SplitScreenQuad;

// ============================================================================
// COMPONENTES
// ============================================================================

#[derive(Component)]
struct RemotePlayer {
    id: u32,
    name: String,
    team_index: u8,
    kick_charge: Vec2, // x = potencia, y = curva
    is_sliding: bool,
    not_interacting: bool,
    base_color: Color,
    ball_target_position: Option<Vec2>,
    stamin_charge: f32,
    active_movement: Option<PlayerMovement>,
    mode_cube_active: bool,
}

#[derive(Component)]
struct RemoteBall;

#[derive(Component)]
struct PlayerCamera {
    local_index: u8,
    server_player_id: Option<u32>,
}

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
struct StaminChargeBar;

#[derive(Component)]
struct PlayerNameText;

#[derive(Component)]
struct PlayerSprite {
    parent_id: u32,
}

#[derive(Component)]
struct PlayerOutline;

#[derive(Component)]
struct SlideCubeVisual {
    parent_id: u32,
}

#[derive(Component)]
struct MinimapDot {
    tracks_entity: Entity,
}

#[derive(Component)]
struct MinimapPlayerName {
    tracks_entity: Entity,
}

#[derive(Component)]
struct MinimapFieldBackground;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CurveAction {
    Left,
    Right,
}

#[derive(Component)]
pub struct KeyVisual {
    player_id: u32,
    action: CurveAction,
}

// ============================================================================
// SISTEMAS DE MENÚ
// ============================================================================

fn setup_menu_camera_if_needed(mut commands: Commands, menu_camera: Query<&MenuCamera>) {
    // Solo crear cámara si no existe
    if menu_camera.is_empty() {
        commands.spawn((Camera2d, MenuCamera));
    }
}

fn cleanup_menu_camera(mut commands: Commands, menu_camera: Query<Entity, With<MenuCamera>>) {
    for entity in menu_camera.iter() {
        commands.entity(entity).despawn();
    }
}

/// Carga los assets embebidos en memoria al iniciar la aplicación
fn load_embedded_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut shaders: ResMut<Assets<Shader>>,
) {
    let ball_image = Image::from_buffer(
        BALL_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .expect("Failed to load embedded ball.png");

    let ball_handle = images.add(ball_image);

    // Cargar el shader de split-screen embebido
    shaders.insert(
        &SPLIT_SCREEN_SHADER_HANDLE,
        Shader::from_wgsl(SPLIT_SCREEN_SHADER_SRC, file!()),
    );

    commands.insert_resource(EmbeddedAssets {
        ball_texture: ball_handle,
    });

    println!("✅ Assets embebidos cargados en memoria");
}

fn menu_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<ConnectionConfig>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);

            ui.heading(egui::RichText::new("🐐⚽ kinetic-ball ⚽🐐").size(48.0));
            ui.add_space(40.0);

            // Contenedor para los campos
            egui::Frame::new().inner_margin(20.0).show(ui, |ui| {
                ui.set_width(400.0);

                ui.horizontal(|ui| {
                    ui.label("Servidor:");
                    ui.add_sized(
                        [270.0, 24.0],
                        egui::TextEdit::singleline(&mut config.server_host),
                    );
                    if ui.button("📋").on_hover_text("Pegar").clicked() {
                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => match clipboard.get_text() {
                                Ok(text) => {
                                    let trimmed = text.trim().to_string();
                                    println!("📋 Pegando servidor: {}", trimmed);
                                    config.server_host = trimmed;
                                }
                                Err(e) => println!("❌ Error obteniendo texto: {:?}", e),
                            },
                            Err(e) => println!("❌ Error creando clipboard: {:?}", e),
                        }
                    }
                });
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Nombre:");
                    ui.add_sized(
                        [270.0, 24.0],
                        egui::TextEdit::singleline(&mut config.player_name),
                    );
                    if ui.button("📋").on_hover_text("Pegar").clicked() {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            if let Ok(text) = clipboard.get_text() {
                                config.player_name = text.trim().to_string();
                            }
                        }
                    }
                });
            });

            // Botones
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(40.0);

                // Botón Ver Salas
                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Ver Salas").size(20.0)),
                    )
                    .clicked()
                {
                    println!("📋 Buscando salas en {}", config.server_host);
                    next_state.set(AppState::RoomSelection);
                }

                ui.add_space(20.0);

                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Ver Players").size(20.0)),
                    )
                    .clicked()
                {
                    println!("📋 Configurando players");
                    next_state.set(AppState::LocalPlayersSetup);
                }

                ui.add_space(20.0);

                // Botón Crear Sala
                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Crear Sala").size(20.0)),
                    )
                    .clicked()
                {
                    println!("🏗️ Crear nueva sala");
                    next_state.set(AppState::CreateRoom);
                }
            });
        });
    });
}

/// Sistema de UI para configuración de teclas
fn settings_ui(
    mut contexts: EguiContexts,
    mut keybindings: ResMut<KeyBindingsConfig>,
    mut ui_state: ResMut<SettingsUIState>,
    mut next_state: ResMut<NextState<AppState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    // Inicializar pending_bindings si es necesario
    if ui_state.pending_bindings.is_none() {
        ui_state.pending_bindings = Some(keybindings.clone());
    }

    // Capturar tecla si estamos en modo rebind
    if let Some(action) = ui_state.rebinding_action {
        for key in keyboard.get_just_pressed() {
            if *key == KeyCode::Escape {
                ui_state.cancel_rebind();
            } else {
                if let Some(ref mut pending) = ui_state.pending_bindings {
                    pending.set_key(action, *key);
                }
                ui_state.rebinding_action = None;
                ui_state.status_message = Some(format!(
                    "'{}' asignado a {}",
                    action.display_name(),
                    key_code_display_name(*key)
                ));
            }
            break;
        }
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(egui::RichText::new("Configuración de Teclas").size(36.0));
            ui.add_space(20.0);

            // Mensaje de estado
            if let Some(ref msg) = ui_state.status_message {
                ui.label(
                    egui::RichText::new(msg)
                        .size(16.0)
                        .color(egui::Color32::YELLOW),
                );
                ui.add_space(10.0);
            }

            // Grid de keybindings
            egui::Frame::new().inner_margin(20.0).show(ui, |ui| {
                egui::Grid::new("keybindings_grid")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .show(ui, |ui| {
                        let pending = ui_state
                            .pending_bindings
                            .clone()
                            .unwrap_or_else(|| keybindings.clone());

                        for action in GameAction::all() {
                            // Nombre de la acción
                            ui.label(egui::RichText::new(action.display_name()).size(18.0));

                            // Botón con tecla actual
                            let key = pending.get_key(*action);
                            let is_rebinding = ui_state.rebinding_action == Some(*action);

                            let button_text = if is_rebinding {
                                "Presiona una tecla...".to_string()
                            } else {
                                key_code_display_name(key)
                            };

                            let button =
                                egui::Button::new(egui::RichText::new(&button_text).size(16.0));

                            if ui.add_sized([150.0, 28.0], button).clicked()
                                && !ui_state.is_rebinding()
                            {
                                ui_state.start_rebind(*action);
                            }

                            ui.end_row();
                        }
                    });
            });

            ui.add_space(30.0);

            // Botones de acción
            ui.horizontal(|ui| {
                // Guardar
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Guardar").size(18.0)),
                    )
                    .clicked()
                {
                    println!("[Settings] Botón Guardar clickeado");
                    if let Some(ref pending) = ui_state.pending_bindings {
                        println!(
                            "[Settings] Aplicando keybindings: kick={:?}",
                            pending.kick.0
                        );
                        *keybindings = pending.clone();
                        match save_keybindings(&keybindings) {
                            Ok(_) => {
                                println!("[Settings] Guardado exitoso");
                                ui_state.status_message =
                                    Some("Configuración guardada".to_string());
                            }
                            Err(e) => {
                                println!("[Settings] Error al guardar: {}", e);
                                ui_state.status_message = Some(format!("Error al guardar: {}", e));
                            }
                        }
                    } else {
                        println!("[Settings] pending_bindings es None!");
                    }
                }

                ui.add_space(15.0);

                // Restaurar defaults
                if ui
                    .add_sized(
                        [180.0, 40.0],
                        egui::Button::new(egui::RichText::new("Restaurar Defaults").size(18.0)),
                    )
                    .clicked()
                {
                    ui_state.pending_bindings = Some(KeyBindingsConfig::default());
                    ui_state.status_message = Some("Restaurado a valores por defecto".to_string());
                }

                ui.add_space(15.0);

                // Volver
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Volver").size(18.0)),
                    )
                    .clicked()
                {
                    ui_state.rebinding_action = None;
                    ui_state.pending_bindings = None;
                    ui_state.status_message = None;
                    next_state.set(AppState::LocalPlayersSetup);
                }
            });
        });
    });
}

// ============================================================================
// LOCAL PLAYERS SETUP SYSTEMS
// ============================================================================

/// Sistema de UI para configurar jugadores locales
fn local_players_setup_ui(
    mut contexts: EguiContexts,
    mut local_players: ResMut<LocalPlayers>,
    available_devices: Res<AvailableInputDevices>,
    mut ui_state: ResMut<LocalPlayersUIState>,
    config: Res<ConnectionConfig>,
    mut next_state: ResMut<NextState<AppState>>,
    gilrs: Option<Res<GilrsWrapper>>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    mut gamepad_config_ui_state: ResMut<GamepadConfigUIState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(egui::RichText::new("Configurar Jugadores Locales").size(36.0));
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Agrega jugadores y asigna dispositivos de entrada")
                    .size(14.0)
                    .color(egui::Color32::GRAY),
            );
            ui.add_space(20.0);

            // Mensaje de estado
            if let Some(ref msg) = ui_state.status_message {
                ui.label(
                    egui::RichText::new(msg)
                        .size(14.0)
                        .color(egui::Color32::YELLOW),
                );
                ui.add_space(10.0);
            }

            // Sección: Agregar nuevo jugador
            ui.group(|ui| {
                ui.set_width(500.0);
                ui.heading("Agregar Jugador");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Nombre:");
                    ui.add_sized(
                        [200.0, 24.0],
                        egui::TextEdit::singleline(&mut ui_state.new_player_name)
                            .hint_text(format!("Jugador {}", local_players.count() + 1)),
                    );
                });

                ui.add_space(5.0);

                // Obtener dispositivos disponibles
                let available = available_devices.get_available_devices(&local_players);

                if available.is_empty() {
                    ui.label(
                        egui::RichText::new("No hay dispositivos disponibles")
                            .color(egui::Color32::RED),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Dispositivo:");
                        egui::ComboBox::from_id_salt("device_selector")
                            .selected_text(
                                available
                                    .get(ui_state.selected_device_index)
                                    .map(|(_, name)| name.as_str())
                                    .unwrap_or("Seleccionar..."),
                            )
                            .show_ui(ui, |ui| {
                                for (i, (_, name)) in available.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut ui_state.selected_device_index,
                                        i,
                                        name,
                                    );
                                }
                            });
                    });

                    ui.add_space(10.0);

                    let can_add = local_players.count() < local_players.max_players as usize
                        && ui_state.selected_device_index < available.len();

                    if ui
                        .add_enabled(
                            can_add,
                            egui::Button::new(egui::RichText::new("+ Agregar Jugador").size(16.0)),
                        )
                        .clicked()
                    {
                        if let Some((device, _)) = available.get(ui_state.selected_device_index) {
                            let name = if ui_state.new_player_name.trim().is_empty() {
                                format!("Jugador {}", local_players.count() + 1)
                            } else {
                                ui_state.new_player_name.trim().to_string()
                            };

                            match local_players.add_player(
                                name.clone(),
                                device.clone(),
                                gilrs.as_deref(),
                            ) {
                                Ok(idx) => {
                                    ui_state.status_message =
                                        Some(format!("Jugador '{}' agregado ({})", name, idx + 1));
                                    ui_state.new_player_name.clear();
                                    ui_state.selected_device_index = 0;
                                }
                                Err(e) => {
                                    ui_state.status_message = Some(e.to_string());
                                }
                            }
                        }
                    }
                }
            });

            ui.add_space(20.0);

            // Sección: Lista de jugadores configurados
            ui.group(|ui| {
                ui.set_width(500.0);
                ui.heading(format!(
                    "Jugadores Configurados ({}/{})",
                    local_players.count(),
                    local_players.max_players
                ));
                ui.add_space(10.0);

                if local_players.is_empty() {
                    ui.label(
                        egui::RichText::new("No hay jugadores configurados")
                            .color(egui::Color32::GRAY),
                    );
                } else {
                    let mut to_remove: Option<u8> = None;
                    let mut to_config: Option<(usize, String)> = None;

                    let mut go_to_keyboard_config = false;

                    for (idx, player) in local_players.players.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}.", player.local_index + 1))
                                    .size(16.0)
                                    .strong(),
                            );
                            ui.label(egui::RichText::new(&player.name).size(16.0));
                            ui.label(
                                egui::RichText::new(format!(
                                    "[{}]",
                                    player.input_device.display_name(&available_devices)
                                ))
                                .size(14.0)
                                .color(egui::Color32::LIGHT_BLUE),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("X").clicked() {
                                        to_remove = Some(player.local_index);
                                    }

                                    // Botón de configuración según tipo de dispositivo
                                    match &player.input_device {
                                        InputDevice::Keyboard => {
                                            if ui
                                                .button("⚙")
                                                .on_hover_text("Configurar teclas")
                                                .clicked()
                                            {
                                                go_to_keyboard_config = true;
                                            }
                                        }
                                        InputDevice::RawGamepad(_) => {
                                            if let Some(ref gamepad_type) = player.gamepad_type_name
                                            {
                                                if ui
                                                    .button("⚙")
                                                    .on_hover_text("Configurar controles")
                                                    .clicked()
                                                {
                                                    to_config = Some((idx, gamepad_type.clone()));
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                },
                            );
                        });
                    }

                    if let Some(idx) = to_remove {
                        local_players.remove_player(idx);
                        ui_state.status_message = Some("Jugador eliminado".to_string());
                    }

                    // Manejar clic en configuración de gamepad
                    if let Some((player_idx, gamepad_type)) = to_config {
                        let current_bindings = gamepad_bindings_map.get_bindings(&gamepad_type);
                        gamepad_config_ui_state.start_config(
                            player_idx,
                            gamepad_type,
                            current_bindings,
                        );
                        next_state.set(AppState::GamepadConfig);
                    }

                    // Manejar clic en configuración de teclado
                    if go_to_keyboard_config {
                        next_state.set(AppState::Settings);
                    }
                }
            });

            ui.add_space(10.0);

            // Información de gamepads detectados
            ui.label(
                egui::RichText::new(format!(
                    "Gamepads detectados: {}",
                    available_devices.gamepads.len()
                ))
                .size(12.0)
                .color(egui::Color32::GRAY),
            );

            ui.add_space(30.0);

            // Botones de acción
            ui.horizontal(|ui| {
                // Volver
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Volver").size(18.0)),
                    )
                    .clicked()
                {
                    next_state.set(AppState::Menu);
                }

                ui.add_space(20.0);

                // Continuar (ir a selección de sala)
                let can_continue = !local_players.is_empty();
                if ui
                    .add_enabled(
                        can_continue,
                        egui::Button::new(egui::RichText::new("Continuar").size(18.0)),
                    )
                    .clicked()
                {
                    println!(
                        "🎮 {} jugadores locales configurados, buscando salas...",
                        local_players.count()
                    );
                    next_state.set(AppState::RoomSelection);
                }
            });
        });
    });
}

// ============================================================================
// GAMEPAD CONFIG SYSTEMS
// ============================================================================

/// Sistema de UI para configuración de gamepad
fn gamepad_config_ui(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<GamepadConfigUIState>,
    mut gamepad_bindings_map: ResMut<GamepadBindingsMap>,
    mut next_state: ResMut<NextState<AppState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut detected_event: ResMut<DetectedGamepadEvent>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    // Capturar input de gamepad si estamos en modo rebind
    if let Some(action) = ui_state.rebinding_action {
        // ESC cancela el rebinding
        if keyboard.just_pressed(KeyCode::Escape) {
            ui_state.cancel_rebind();
            detected_event.input = None; // Limpiar evento
        } else if let Some((_gamepad_id, raw_input)) = detected_event.input.take() {
            // Se detectó un input de gamepad - take() consume y limpia el evento
            println!(
                "🎮 [UI] Asignando {:?} a '{}'",
                raw_input,
                action.display_name()
            );
            if let Some(ref mut pending) = ui_state.pending_bindings {
                pending.set_binding(action, Some(raw_input));
            }
            ui_state.last_detected_input = Some(raw_input);
            ui_state.rebinding_action = None;
            ui_state.status_message = Some(format!(
                "'{}' asignado a {}",
                action.display_name(),
                raw_input.display_name()
            ));
        }
    } else {
        // Si no estamos en rebinding, limpiar cualquier evento pendiente
        detected_event.input = None;
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);

            // Título con nombre del gamepad
            let gamepad_name = ui_state
                .gamepad_type_name
                .clone()
                .unwrap_or_else(|| "Gamepad".to_string());
            ui.heading(egui::RichText::new(format!("Configurar: {}", gamepad_name)).size(32.0));
            ui.add_space(20.0);

            // Mensaje de estado
            if let Some(ref msg) = ui_state.status_message {
                ui.label(
                    egui::RichText::new(msg)
                        .size(16.0)
                        .color(egui::Color32::YELLOW),
                );
                ui.add_space(10.0);
            }

            // Grid de bindings
            egui::Frame::new().inner_margin(20.0).show(ui, |ui| {
                egui::Grid::new("gamepad_bindings_grid")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .show(ui, |ui| {
                        let pending = ui_state.pending_bindings.clone().unwrap_or_default();

                        for action in GameAction::all() {
                            // Nombre de la acción
                            ui.label(egui::RichText::new(action.display_name()).size(18.0));

                            // Botón con binding actual
                            let binding = pending.get_binding(*action);
                            let is_rebinding = ui_state.rebinding_action == Some(*action);

                            let button_text = if is_rebinding {
                                "Presiona botón/eje...".to_string()
                            } else {
                                binding
                                    .map(|b| b.display_name())
                                    .unwrap_or_else(|| "Sin asignar".to_string())
                            };

                            let button =
                                egui::Button::new(egui::RichText::new(&button_text).size(16.0));

                            if ui.add_sized([180.0, 28.0], button).clicked()
                                && !ui_state.is_rebinding()
                            {
                                ui_state.start_rebind(*action);
                            }

                            ui.end_row();
                        }
                    });
            });

            ui.add_space(30.0);

            // Botones de acción
            ui.horizontal(|ui| {
                // Guardar
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Guardar").size(18.0)),
                    )
                    .clicked()
                {
                    if let (Some(ref gamepad_type), Some(ref pending)) =
                        (&ui_state.gamepad_type_name, &ui_state.pending_bindings)
                    {
                        gamepad_bindings_map.set_bindings(gamepad_type.clone(), pending.clone());
                        match save_gamepad_bindings_map(&gamepad_bindings_map) {
                            Ok(_) => {
                                ui_state.status_message =
                                    Some("Configuración guardada".to_string());
                            }
                            Err(e) => {
                                ui_state.status_message = Some(format!("Error al guardar: {}", e));
                            }
                        }
                    }
                }

                ui.add_space(15.0);

                // Restaurar defaults
                if ui
                    .add_sized(
                        [180.0, 40.0],
                        egui::Button::new(egui::RichText::new("Restaurar Defaults").size(18.0)),
                    )
                    .clicked()
                {
                    ui_state.pending_bindings = Some(GamepadBindingsConfig::default());
                    ui_state.status_message = Some("Restaurado a valores por defecto".to_string());
                }

                ui.add_space(15.0);

                // Volver
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Volver").size(18.0)),
                    )
                    .clicked()
                {
                    ui_state.reset();
                    next_state.set(AppState::LocalPlayersSetup);
                }
            });

            ui.add_space(20.0);

            // Instrucciones
            ui.label(
                egui::RichText::new(
                    "Haz clic en una acción y luego presiona un botón o mueve un eje del gamepad",
                )
                .size(12.0)
                .color(egui::Color32::GRAY),
            );
        });
    });
}

// ============================================================================
// ROOM SELECTION SYSTEMS
// ============================================================================

fn fetch_rooms(
    config: Res<ConnectionConfig>,
    mut room_list: ResMut<RoomList>,
    mut fetch_channel: ResMut<RoomFetchChannel>,
) {
    room_list.loading = true;
    room_list.error = None;
    room_list.rooms.clear();

    let (tx, rx) = mpsc::channel();
    fetch_channel.receiver = Some(Arc::new(Mutex::new(rx)));

    let url = format!("{}/api/rooms", config.http_url());
    println!("🌐 Fetching rooms from: {}", url);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        let result = rt.block_on(async {
            let client = reqwest::Client::new();
            match client
                .get(&url)
                .header("ngrok-skip-browser-warning", "true")
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json::<Vec<RoomInfo>>().await {
                            Ok(rooms) => Ok(rooms),
                            Err(e) => Err(format!("Error parsing response: {}", e)),
                        }
                    } else {
                        let body = response.text().await.unwrap_or_default();
                        Err(format!("Server error: {} - Body: {}", status, body))
                    }
                }
                Err(e) => Err(format!("Connection error: {}", e)),
            }
        });

        let _ = tx.send(result);
    });
}

fn check_rooms_fetch(mut room_list: ResMut<RoomList>, mut fetch_channel: ResMut<RoomFetchChannel>) {
    let result = if let Some(ref rx) = fetch_channel.receiver {
        if let Ok(guard) = rx.lock() {
            guard.try_recv().ok()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(result) = result {
        match result {
            Ok(rooms) => {
                println!("📋 {} salas encontradas", rooms.len());
                room_list.rooms = rooms;
                room_list.loading = false;
            }
            Err(e) => {
                println!("❌ Error fetching rooms: {}", e);
                room_list.error = Some(e);
                room_list.loading = false;
            }
        }
        fetch_channel.receiver = None;
    }
}

fn room_selection_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<ConnectionConfig>,
    mut room_list: ResMut<RoomList>,
    mut selected_room: ResMut<SelectedRoom>,
    mut next_state: ResMut<NextState<AppState>>,
    mut fetch_channel: ResMut<RoomFetchChannel>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(egui::RichText::new("Salas Disponibles").size(36.0));
            ui.add_space(20.0);

            // Botones superiores
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [100.0, 30.0],
                        egui::Button::new(egui::RichText::new("← Volver").size(16.0)),
                    )
                    .clicked()
                {
                    next_state.set(AppState::Menu);
                }

                ui.add_space(20.0);

                let refresh_enabled = !room_list.loading;
                if ui
                    .add_enabled(
                        refresh_enabled,
                        egui::Button::new(egui::RichText::new("🔄 Actualizar").size(16.0)),
                    )
                    .clicked()
                {
                    // Trigger refresh
                    room_list.loading = true;
                    room_list.error = None;

                    let url = format!("{}/api/rooms", config.http_url());

                    let (tx, rx) = mpsc::channel();
                    fetch_channel.receiver = Some(Arc::new(Mutex::new(rx)));

                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("Failed to create tokio runtime");

                        let result = rt.block_on(async {
                            let client = reqwest::Client::new();
                            match client
                                .get(&url)
                                .header("ngrok-skip-browser-warning", "true")
                                .send()
                                .await
                            {
                                Ok(response) => {
                                    if response.status().is_success() {
                                        match response.json::<Vec<RoomInfo>>().await {
                                            Ok(rooms) => Ok(rooms),
                                            Err(e) => Err(format!("Error parsing response: {}", e)),
                                        }
                                    } else {
                                        Err(format!("Server error: {}", response.status()))
                                    }
                                }
                                Err(e) => Err(format!("Connection error: {}", e)),
                            }
                        });

                        let _ = tx.send(result);
                    });
                }
            });

            ui.add_space(20.0);

            // Estado de carga o error
            if room_list.loading {
                ui.spinner();
                ui.label("Cargando salas...");
            } else if let Some(ref error) = room_list.error {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
            }

            ui.add_space(10.0);

            // Lista de salas
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    if room_list.rooms.is_empty() && !room_list.loading {
                        ui.label("No hay salas disponibles");
                    }

                    for room in &room_list.rooms {
                        let is_selected = selected_room.room_id.as_ref() == Some(&room.room_id);
                        let is_full = matches!(room.status, RoomStatus::Full);

                        let frame = if is_selected {
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(60, 80, 120))
                                .inner_margin(10.0)
                                .corner_radius(5.0)
                        } else {
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(40, 40, 50))
                                .inner_margin(10.0)
                                .corner_radius(5.0)
                        };

                        frame.show(ui, |ui| {
                            ui.set_width(500.0);

                            let response = ui.interact(
                                ui.max_rect(),
                                ui.id().with(&room.room_id),
                                egui::Sense::click(),
                            );

                            ui.horizontal(|ui| {
                                // Nombre de la sala
                                ui.label(egui::RichText::new(&room.name).size(18.0).strong());

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        // Status
                                        let (status_text, status_color) = match room.status {
                                            RoomStatus::Open => ("Abierta", egui::Color32::GREEN),
                                            RoomStatus::Full => ("Llena", egui::Color32::RED),
                                            RoomStatus::Closed => ("Cerrada", egui::Color32::GRAY),
                                        };
                                        ui.colored_label(status_color, status_text);

                                        // Jugadores
                                        ui.label(format!(
                                            "{}/{}",
                                            room.current_players, room.max_players
                                        ));
                                    },
                                );
                            });

                            // Info adicional
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("ID: {}", room.room_id))
                                        .size(12.0)
                                        .color(egui::Color32::GRAY),
                                );
                                if let Some(ref map) = room.map_name {
                                    ui.label(
                                        egui::RichText::new(format!("Mapa: {}", map))
                                            .size(12.0)
                                            .color(egui::Color32::GRAY),
                                    );
                                }
                                if let Some(ref version) = room.min_version {
                                    ui.label(
                                        egui::RichText::new(format!("v{}", version))
                                            .size(12.0)
                                            .color(egui::Color32::LIGHT_BLUE),
                                    );
                                }
                            });

                            // Handle clicks
                            if response.clicked() {
                                selected_room.room_id = Some(room.room_id.clone());
                            }

                            if response.double_clicked() && !is_full {
                                config.room = room.room_id.clone();
                                println!("🎮 Entrando a sala: {}", room.room_id);
                                next_state.set(AppState::Connecting);
                            }
                        });

                        ui.add_space(5.0);
                    }
                });

            ui.add_space(20.0);

            // Botón de entrar (alternativa a doble click)
            let can_join = selected_room.room_id.is_some()
                && room_list.rooms.iter().any(|r| {
                    Some(&r.room_id) == selected_room.room_id.as_ref()
                        && !matches!(r.status, RoomStatus::Full)
                });

            if ui
                .add_enabled(
                    can_join,
                    egui::Button::new(egui::RichText::new("Entrar a la Sala").size(18.0)),
                )
                .clicked()
            {
                if let Some(ref room_id) = selected_room.room_id {
                    config.room = room_id.clone();
                    println!("🎮 Entrando a sala: {}", room_id);
                    next_state.set(AppState::Connecting);
                }
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Doble click en una sala para entrar")
                    .size(12.0)
                    .color(egui::Color32::GRAY),
            );
        });
    });
}

fn create_room_ui(
    mut contexts: EguiContexts,
    mut create_config: ResMut<CreateRoomConfig>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(egui::RichText::new("Crear Sala").size(36.0));
            ui.add_space(20.0);

            // Botón volver
            if ui
                .add_sized(
                    [100.0, 30.0],
                    egui::Button::new(egui::RichText::new("← Volver").size(16.0)),
                )
                .clicked()
            {
                next_state.set(AppState::Menu);
            }

            ui.add_space(30.0);

            // Formulario
            ui.group(|ui| {
                ui.set_width(400.0);
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Nombre de la sala:");
                    ui.add_sized(
                        [250.0, 24.0],
                        egui::TextEdit::singleline(&mut create_config.room_name),
                    );
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Máximo de jugadores:");
                    ui.add(egui::Slider::new(&mut create_config.max_players, 2..=16));
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Mapa:");
                    ui.add_sized(
                        [200.0, 24.0],
                        egui::TextEdit::singleline(&mut create_config.map_path)
                            .hint_text("(embebido por defecto)"),
                    );
                    if ui.button("📂").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Mapas", &["json5", "json", "hbs"])
                            .set_directory("maps")
                            .pick_file()
                        {
                            create_config.map_path = path.display().to_string();
                        }
                    }
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Escala del mapa:");
                    ui.add(egui::Slider::new(&mut create_config.scale, 0.5..=2.0).step_by(0.1));
                });

                ui.add_space(10.0);
            });

            ui.add_space(30.0);

            // Botón crear
            if ui
                .add_sized(
                    [200.0, 50.0],
                    egui::Button::new(egui::RichText::new("🏗️ Crear y Hostear").size(20.0)),
                )
                .clicked()
            {
                println!("🏗️ Creando sala: {}", create_config.room_name);
                next_state.set(AppState::HostingRoom);
            }
        });
    });
}

fn start_hosting(config: Res<ConnectionConfig>, create_config: Res<CreateRoomConfig>) {
    let server_host = config.server_host.clone();
    let room_name = create_config.room_name.clone();
    let max_players = create_config.max_players;
    let map_path = if create_config.map_path.is_empty() {
        None
    } else {
        Some(create_config.map_path.clone())
    };
    let scale = create_config.scale;

    // Generar room_id único
    let room_id = format!(
        "room_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    println!("🚀 Iniciando host...");
    println!("   Sala: {}", room_name);
    println!("   Room ID: {}", room_id);
    println!("   Max jugadores: {}", max_players);

    // Lanzar host en thread separado
    std::thread::spawn(move || {
        host::host(
            map_path,
            DEFAULT_MAP,
            scale,
            room_id,
            server_host,
            room_name,
            max_players,
        );
    });
}

fn hosting_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<AppState>>,
    create_config: Res<CreateRoomConfig>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.heading(egui::RichText::new("🎮 Sala Activa").size(36.0));
            ui.add_space(20.0);

            ui.label(egui::RichText::new(format!("Sala: {}", create_config.room_name)).size(24.0));
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!("Jugadores máximos: {}", create_config.max_players))
                    .size(18.0)
                    .color(egui::Color32::GRAY),
            );

            ui.add_space(30.0);
            ui.label(
                egui::RichText::new("El servidor está corriendo en segundo plano.").size(16.0),
            );
            ui.label(
                egui::RichText::new("Los jugadores pueden unirse desde 'Ver Salas'.").size(16.0),
            );

            ui.add_space(50.0);

            if ui
                .add_sized(
                    [200.0, 50.0],
                    egui::Button::new(egui::RichText::new("← Volver al Menú").size(20.0)),
                )
                .clicked()
            {
                next_state.set(AppState::Menu);
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Nota: El servidor seguirá activo aunque vuelvas al menú")
                    .size(12.0)
                    .color(egui::Color32::GRAY),
            );
        });
    });
}

fn start_connection(
    config: Res<ConnectionConfig>,
    mut channels: ResMut<NetworkChannels>,
    local_players: Res<LocalPlayers>,
) {
    let (network_tx, network_rx) = mpsc::channel();
    let (input_tx, input_rx) = mpsc::channel();

    // Guardar los canales
    channels.receiver = Some(Arc::new(Mutex::new(network_rx)));
    channels.sender = Some(input_tx);

    let ws_url = config.ws_url();
    let room = config.room.clone();

    // Recoger los nombres de los jugadores locales
    // Si no hay jugadores locales configurados, usar el nombre del config (modo legacy)
    let player_names: Vec<String> = if local_players.is_empty() {
        vec![config.player_name.clone()]
    } else {
        local_players
            .players
            .iter()
            .map(|p| p.name.clone())
            .collect()
    };

    println!(
        "🌐 [Red] Iniciando conexión con {} jugadores locales",
        player_names.len()
    );

    // Iniciar hilo de red
    std::thread::spawn(move || {
        println!("🌐 [Red] Iniciando cliente WebRTC");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Fallo al crear Runtime de Tokio");

        rt.block_on(async {
            start_webrtc_client(ws_url, room, player_names, network_tx, input_rx).await;
        });
        println!("🌐 [Red] El hilo de red HA TERMINADO");
    });
}

fn check_connection(channels: Res<NetworkChannels>, mut next_state: ResMut<NextState<AppState>>) {
    // Verificar si hemos recibido el WELCOME
    if let Some(ref receiver) = channels.receiver {
        if let Ok(rx) = receiver.lock() {
            // Peek sin consumir - si hay mensajes, la conexión está lista
            // En realidad, simplemente pasamos a InGame y dejamos que process_network_messages maneje los mensajes
            drop(rx);
            // Por simplicidad, pasamos directamente a InGame después de un frame
            next_state.set(AppState::InGame);
        }
    }
}

// ============================================================================
// NETWORK CLIENT (Matchbox WebRTC)
// ============================================================================

async fn start_webrtc_client(
    server_url: String,
    room: String,
    player_names: Vec<String>,
    network_tx: mpsc::Sender<ServerMessage>,
    input_rx: mpsc::Receiver<(u32, PlayerInput)>,
) {
    // Conectar al proxy
    let room_url = format!("{}/{}", server_url, room);
    println!("🔌 [Red] Conectando a {}", room_url);

    // Crear WebRtcSocket y conectar a la room
    let (mut socket, loop_fut) = WebRtcSocket::builder(room_url)
        .add_channel(matchbox_socket::ChannelConfig::reliable()) // Canal 0: Control
        .add_channel(matchbox_socket::ChannelConfig::unreliable()) // Canal 1: GameData
        .build();

    // Spawn el loop de matchbox
    tokio::spawn(loop_fut);

    println!(
        "✅ [Red] WebRTC socket creado, esperando conexión con peers... ({} jugadores locales)",
        player_names.len()
    );

    // El server_peer_id real se determina cuando recibimos WELCOME
    let mut server_peer_id: Option<matchbox_socket::PeerId> = None;

    // Rastrear a qué peers ya enviamos JOINs
    let mut peers_joined: std::collections::HashSet<matchbox_socket::PeerId> =
        std::collections::HashSet::new();

    // Contador de WELCOMEs recibidos para asociar con local_index
    let mut welcomes_received: usize = 0;

    // Loop principal: recibir mensajes y enviar inputs
    loop {
        // Procesar nuevos peers y enviar JOINs para todos los jugadores locales
        socket.update_peers();
        let current_peers: Vec<_> = socket.connected_peers().collect();

        for peer_id in current_peers {
            if !peers_joined.contains(&peer_id) {
                // Nuevo peer, enviar JOIN para cada jugador local
                for (idx, name) in player_names.iter().enumerate() {
                    let client_version = ProtocolVersion::current();
                    let join_msg = ControlMessage::Join {
                        player_name: name.clone(),
                        client_version: Some(client_version),
                    };
                    if let Ok(data) = bincode::serialize(&join_msg) {
                        println!(
                            "📤 [Red] Enviando JOIN #{} ({}) v{} a peer {:?}...",
                            idx + 1,
                            name,
                            client_version,
                            peer_id
                        );
                        socket.channel_mut(0).send(data.into(), peer_id);
                    }
                }
                peers_joined.insert(peer_id);
            }
        }

        // Recibir mensajes del servidor
        // Canal 0: Control messages (reliable)
        for (peer_id, packet) in socket.channel_mut(0).receive() {
            if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                match msg {
                    ControlMessage::Welcome { player_id, map } => {
                        println!(
                            "🎉 [Red] WELCOME #{} recibido de peer {:?}! Player ID: {}",
                            welcomes_received + 1,
                            peer_id,
                            player_id
                        );

                        // Guardar el peer_id del servidor real (del primer WELCOME)
                        if server_peer_id.is_none() {
                            server_peer_id = Some(peer_id);
                        }

                        // Convertir a ServerMessage para compatibilidad con el código existente
                        let server_msg = ServerMessage::Welcome { player_id, map };
                        let _ = network_tx.send(server_msg);

                        // Enviar READY al servidor real
                        let ready_msg = ControlMessage::Ready;
                        if let Ok(data) = bincode::serialize(&ready_msg) {
                            println!(
                                "📤 [Red -> Servidor] Enviando READY para jugador {}...",
                                player_id
                            );
                            socket.channel_mut(0).send(data.into(), peer_id);
                        }

                        welcomes_received += 1;
                    }
                    ControlMessage::PlayerDisconnected { player_id } => {
                        println!("👋 [Red] Jugador {} se desconectó", player_id);
                        let _ = network_tx.send(ServerMessage::PlayerDisconnected { player_id });
                    }
                    ControlMessage::VersionMismatch {
                        client_version,
                        min_required,
                        message,
                    } => {
                        println!(
                            "❌ [Red] VERSION INCOMPATIBLE: Tu versión {} es menor que la mínima requerida {}",
                            client_version, min_required
                        );
                        println!("   {}", message);
                        let _ = network_tx.send(ServerMessage::Error {
                            message: format!(
                                "Versión incompatible: tienes v{}, se requiere v{} o superior. {}",
                                client_version, min_required, message
                            ),
                        });
                    }
                    ControlMessage::Error { message } => {
                        println!("❌ [Red] Error del servidor: {}", message);
                        let _ = network_tx.send(ServerMessage::Error { message });
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

        // Enviar inputs desde Bevy (solo si ya identificamos al servidor)
        if let Some(server_id) = server_peer_id {
            while let Ok((player_id, input)) = input_rx.try_recv() {
                let input_msg = GameDataMessage::Input { player_id, input };
                if let Ok(data) = bincode::serialize(&input_msg) {
                    socket.channel_mut(1).send(data.into(), server_id); // Canal 1 = unreliable
                }
            }
        } else {
            // Descartar inputs hasta que tengamos servidor
            while input_rx.try_recv().is_ok() {}
        }

        // Pequeña pausa
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }
}

// ============================================================================
// GAME SYSTEMS
// ============================================================================

fn spawn_key_visual_2d(
    parent: &mut ChildSpawnerCommands,
    key_text: &str,
    player_id: u32,
    action: CurveAction,
    translation: Vec3,
    rotation: Quat,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    render_layers: RenderLayers,
) {
    let font_size = match key_text.len() {
        1 => 32.0,
        2 => 28.0,
        _ => 22.0,
    };

    let key_width = (key_text.len() as f32 * 20.0).max(50.0);
    let key_height = 50.0;

    parent
        .spawn((
            KeyVisual { player_id, action },
            Transform {
                translation,
                rotation, // <--- Aplicamos la rotación aquí
                ..default()
            },
            Visibility::default(),
            render_layers.clone(),
        ))
        .with_children(|key| {
            // SOMBRA (Fija en la base)
            key.spawn((
                Mesh2d(meshes.add(Rectangle::new(key_width + 4.0, key_height + 8.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.05, 0.05, 0.05))),
                Transform::from_xyz(0.0, -4.0, -0.1),
                render_layers.clone(),
            ));

            // CUERPO MÓVIL (Lo que se hunde al presionar)
            key.spawn((
                Transform::default(),
                Visibility::default(),
                render_layers.clone(),
            ))
            .with_children(|body| {
                // Borde/Highlight
                body.spawn((
                    Mesh2d(meshes.add(Rectangle::new(key_width + 2.0, key_height + 2.0))),
                    MeshMaterial2d(materials.add(Color::srgb(0.4, 0.4, 0.4))),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                    render_layers.clone(),
                ));

                // Superficie principal
                body.spawn((
                    Mesh2d(meshes.add(Rectangle::new(key_width, key_height))),
                    MeshMaterial2d(materials.add(Color::srgb(0.2, 0.2, 0.2))),
                    Transform::from_xyz(0.0, 0.0, 0.2),
                    render_layers.clone(),
                ));

                // Texto de la tecla
                body.spawn((
                    Text2d::new(key_text),
                    TextFont {
                        font_size,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Transform::from_xyz(0.0, 0.0, 0.3),
                    render_layers.clone(),
                ));
            });
        });
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut split_materials: ResMut<Assets<SplitScreenMaterial>>,
    mut split_textures: ResMut<SplitScreenTextures>,
    local_players: Res<LocalPlayers>,
    windows: Query<&Window>,
) {
    // Obtener tamaño de ventana para calcular viewports
    let window = windows.iter().next();
    // Tamaño físico (para texturas de render target)
    let window_size = window
        .map(|w| UVec2::new(w.physical_width(), w.physical_height()))
        .unwrap_or(UVec2::new(1280, 720));
    // Tamaño lógico (para el quad del compositor, porque ScalingMode::WindowSize usa unidades lógicas)
    let window_logical_size = window
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));

    // Número de jugadores locales (mínimo 1)
    let num_local_players = local_players.players.len().max(1);
    let use_dynamic_split = num_local_players >= 2;

    // Para split-screen dinámico: crear texturas de render target para cada cámara
    let mut player_camera_textures: Vec<Handle<Image>> = Vec::new();

    if use_dynamic_split {
        for i in 0..2 {
            let mut cam_image = Image::new_uninit(
                bevy::render::render_resource::Extent3d {
                    width: window_size.x,
                    height: window_size.y,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Bgra8UnormSrgb,
                RenderAssetUsages::all(),
            );
            cam_image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT;
            let cam_image_handle = images.add(cam_image);
            player_camera_textures.push(cam_image_handle);
        }

        // Guardar referencias a las texturas
        split_textures.camera1_texture = player_camera_textures.first().cloned();
        split_textures.camera2_texture = player_camera_textures.get(1).cloned();
    }

    // Crear cámaras para cada jugador local
    let mut player_cameras: Vec<Entity> = Vec::new();
    for i in 0..num_local_players.min(2) {
        let server_player_id = local_players
            .players
            .get(i)
            .and_then(|lp| lp.server_player_id);

        if use_dynamic_split {
            // Renderizar a textura para composición dinámica
            let target_texture = player_camera_textures.get(i).cloned();
            if let Some(texture) = target_texture {
                let cam = commands
                    .spawn((
                        Camera2d,
                        Camera {
                            order: -(10 + i as isize), // Renderizar antes del compositor
                            target: RenderTarget::Image(texture.into()),
                            clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.15)),
                            ..default()
                        },
                        Projection::Orthographic(OrthographicProjection {
                            scale: 3.0,
                            ..OrthographicProjection::default_2d()
                        }),
                        Transform::from_xyz(0.0, 0.0, 999.0),
                        PlayerCamera {
                            local_index: i as u8,
                            server_player_id,
                        },
                        RenderLayers::layer(0),
                    ))
                    .id();
                player_cameras.push(cam);
            }
        } else {
            // Un solo jugador: renderizar directamente a pantalla
            commands.spawn((
                Camera2d,
                Camera {
                    order: i as isize,
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scale: 3.0,
                    ..OrthographicProjection::default_2d()
                }),
                Transform::from_xyz(0.0, 0.0, 999.0),
                PlayerCamera {
                    local_index: i as u8,
                    server_player_id,
                },
                RenderLayers::layer(0),
            ));
        }
    }

    // Crear compositor de split-screen si hay 2+ jugadores
    if use_dynamic_split {
        if let (Some(tex1), Some(tex2)) = (
            player_camera_textures.first().cloned(),
            player_camera_textures.get(1).cloned(),
        ) {
            // Crear el material de composición
            let split_material = split_materials.add(SplitScreenMaterial {
                camera1_texture: tex1,
                camera2_texture: tex2,
                split_params: Vec4::new(FRAC_PI_2, 0.0, 0.5, 0.5), // angle, factor, center_x, center_y
            });

            // Crear cámara compositor con proyección fija al tamaño de ventana
            commands.spawn((
                Camera2d,
                Camera {
                    order: 0, // Después de las cámaras de jugador, antes de UI
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: window_logical_size.x / 2.0,
                        height: window_logical_size.y / 2.0,
                    },
                    ..OrthographicProjection::default_2d()
                }),
                CompositorCamera,
                RenderLayers::layer(3), // Layer especial para compositor
            ));

            // Quad fullscreen que muestra la composición
            commands.spawn((
                Mesh2d(meshes.add(Rectangle::new(window_logical_size.x, window_logical_size.y))),
                MeshMaterial2d(split_material),
                Transform::from_xyz(0.0, 0.0, 0.0),
                SplitScreenQuad,
                RenderLayers::layer(3),
            ));
        }
    }

    // --- Crear texturas de render target para ViewportNodes ---

    let minimap_size = config.minimap_size;
    let detail_size = config.player_detail_size;

    // Textura para minimapa
    let mut minimap_image = Image::new_uninit(
        bevy::render::render_resource::Extent3d {
            width: minimap_size.x,
            height: minimap_size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::all(),
    );
    minimap_image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let minimap_image_handle = images.add(minimap_image);

    // --- Cámara minimapa - Layer 1 (renderiza a textura) ---
    let minimap_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -2, // Renderiza antes que la principal
                target: RenderTarget::Image(minimap_image_handle.clone().into()),
                clear_color: ClearColorConfig::Custom(Color::srgba(0.1, 0.3, 0.1, 0.6)),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: config.arena_width,
                    height: config.arena_height,
                },
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_xyz(0.0, 0.0, 999.0),
            MinimapCamera,
            RenderLayers::layer(1),
        ))
        .id();

    // --- Crear cámaras de detalle para cada jugador local ---
    let mut detail_cameras: Vec<Entity> = Vec::new();
    for i in 0..num_local_players.min(2) {
        // Crear textura de detalle para este jugador
        let mut detail_image = Image::new_uninit(
            bevy::render::render_resource::Extent3d {
                width: detail_size.x,
                height: detail_size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Bgra8UnormSrgb,
            RenderAssetUsages::all(),
        );
        detail_image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::RENDER_ATTACHMENT;
        let detail_image_handle = images.add(detail_image);

        let detail_camera = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -1 - i as isize, // Renderiza antes que la principal
                    target: RenderTarget::Image(detail_image_handle.clone().into()),
                    clear_color: ClearColorConfig::Custom(Color::srgba(0.2, 0.2, 0.2, 0.6)),
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scale: 1.5,
                    ..OrthographicProjection::default_2d()
                }),
                Transform::from_xyz(0.0, 0.0, 999.0),
                PlayerDetailCamera {
                    local_index: i as u8,
                },
                RenderLayers::layer(2),
            ))
            .id();

        detail_cameras.push(detail_camera);
    }

    // --- Cámara UI dedicada (sin viewport, renderiza UI en pantalla completa) ---
    let ui_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 100, // Renderiza después de todo lo demás
                ..default()
            },
            GameUiCamera,
            // No renderiza nada del juego, solo UI
            RenderLayers::none(),
        ))
        .id();

    // La línea divisoria se dibuja directamente en el shader del compositor

    // --- UI con ViewportNodes ---
    // Contenedor raíz para posicionar los viewports
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                padding: UiRect::all(Val::Px(config.ui_padding)),
                ..default()
            },
            UiTargetCamera(ui_camera),
            // No bloquear clicks en el juego
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // Detalle del jugador 1 (izquierda abajo) - circular
            if let Some(&detail_cam) = detail_cameras.first() {
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BorderRadius::all(Val::Percent(50.0)), // Circular
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                    ViewportNode::new(detail_cam),
                ));
            } else {
                // Espaciador si no hay jugadores
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }

            // Minimapa (centro abajo)
            parent.spawn((
                Node {
                    width: Val::Px(minimap_size.x as f32),
                    height: Val::Px(minimap_size.y as f32),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BorderColor::all(Color::WHITE),
                BorderRadius::all(Val::Px(4.0)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                ViewportNode::new(minimap_camera),
            ));

            // Detalle del jugador 2 (derecha abajo) - circular
            if let Some(&detail_cam) = detail_cameras.get(1) {
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BorderRadius::all(Val::Percent(50.0)), // Circular
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                    ViewportNode::new(detail_cam),
                ));
            } else if num_local_players == 1 {
                // Espaciador para mantener el minimapa centrado cuando hay 1 jugador
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }
        });

    // El Campo de Juego (Césped) - Color verde - Layer 0
    commands.spawn((
        Sprite {
            color: Color::srgb(0.2, 0.5, 0.2),
            custom_size: Some(Vec2::new(config.arena_width, config.arena_height)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0), // Z = 0 (fondo)
        FieldBackground,
        RenderLayers::layer(0),
    ));

    // Las líneas del minimapa se crean dinámicamente cuando se carga el mapa
    // Líneas blancas del campo (bordes)
    let thickness = 5.0;
    let w = config.arena_width;
    let h = config.arena_height;

    // Top
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(w + thickness, thickness)),
            ..default()
        },
        Transform::from_xyz(0.0, h / 2.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Bottom
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(w + thickness, thickness)),
            ..default()
        },
        Transform::from_xyz(0.0, -h / 2.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Left
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(thickness, h + thickness)),
            ..default()
        },
        Transform::from_xyz(-w / 2.0, 0.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Right
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(thickness, h + thickness)),
            ..default()
        },
        Transform::from_xyz(w / 2.0, 0.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // El mensaje Ready ahora se envía automáticamente en el thread de red después de recibir Welcome

    println!("✅ Cliente configurado y campo listo");
}

// Resource para trackear el input anterior (legacy, mantenido por compatibilidad)
#[derive(Resource, Default)]
struct PreviousInput(PlayerInput);

/// Sistema que lee input de todos los jugadores locales y lo envía al servidor
fn handle_multi_player_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    channels: Res<NetworkChannels>,
    local_players: Res<LocalPlayers>,
    my_player_id: Res<MyPlayerId>,
    gilrs: Option<Res<GilrsWrapper>>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    keybindings: Res<KeyBindingsConfig>,
    players: Query<&RemotePlayer>,
) {
    let Some(ref sender) = channels.sender else {
        return;
    };

    // Si no hay jugadores locales configurados, usar modo legacy (un jugador con teclado)
    if local_players.is_empty() {
        let Some(my_id) = my_player_id.0 else {
            return;
        };

        // Verificar si el jugador local está en modo cubo
        let is_cube_mode = players
            .iter()
            .find(|p| p.id == my_id)
            .map(|p| p.mode_cube_active)
            .unwrap_or(false);

        // Leer input del teclado (modo legacy)
        let input = local_players::read_keyboard_input(&keyboard, &keybindings, is_cube_mode);

        // Enviar input con el player_id
        if let Err(e) = sender.send((my_id, input)) {
            println!("⚠️ [Bevy] Error enviando input al canal: {:?}", e);
        }
        return;
    }

    // DEBUG: Log una vez cada 60 frames aproximadamente
    static mut FRAME_COUNT: u32 = 0;
    unsafe {
        FRAME_COUNT += 1;
        if FRAME_COUNT % 120 == 0 {
            println!(
                "🎮 [DEBUG] {} jugadores locales configurados, gamepads en query: {}",
                local_players.players.len(),
                gamepads.iter().count()
            );
            for (i, p) in local_players.players.iter().enumerate() {
                println!(
                    "   Jugador {}: '{}', device={:?}, server_id={:?}",
                    i, p.name, p.input_device, p.server_player_id
                );
            }
        }
    }

    // Iterar sobre cada jugador local y enviar su input
    for local_player in &local_players.players {
        // Solo procesar si tiene un server_player_id asignado
        let Some(server_id) = local_player.server_player_id else {
            continue;
        };

        // Verificar si este jugador está en modo cubo
        let is_cube_mode = players
            .iter()
            .find(|p| p.id == server_id)
            .map(|p| p.mode_cube_active)
            .unwrap_or(false);

        // Leer input según el dispositivo asignado
        let input = read_local_player_input(
            local_player,
            &keyboard,
            &keybindings,
            &gamepads,
            gilrs.as_deref(),
            &gamepad_bindings_map,
            is_cube_mode,
        );

        // Enviar input con el player_id del servidor
        if let Err(e) = sender.send((server_id, input)) {
            println!(
                "⚠️ [Bevy] Error enviando input para jugador {} al canal: {:?}",
                server_id, e
            );
        }
    }
}

use bevy::ecs::system::SystemParam;

#[derive(SystemParam)]
pub struct NetworkParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub embedded_assets: Res<'w, EmbeddedAssets>,
    pub config: ResMut<'w, GameConfig>,
    pub channels: Res<'w, NetworkChannels>,
    pub my_id: ResMut<'w, MyPlayerId>,
    pub loaded_map: ResMut<'w, LoadedMap>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<ColorMaterial>>,
    pub keybindings: Res<'w, KeyBindingsConfig>,
    pub gamepad_bindings_map: Res<'w, GamepadBindingsMap>,
    pub local_players: ResMut<'w, LocalPlayers>,
    pub game_tick: ResMut<'w, GameTick>,
    pub player_colors: ResMut<'w, PlayerColors>,
}

#[derive(SystemParam)]
pub struct NetworkQueries<'w, 's> {
    pub ball_q: Query<
        'w,
        's,
        (
            &'static mut Interpolated,
            &'static mut Transform,
            &'static RemoteBall,
        ),
        Without<RemotePlayer>,
    >,
    pub players_q: Query<
        'w,
        's,
        (
            Entity,
            &'static mut Interpolated,
            &'static mut Transform,
            &'static mut RemotePlayer,
            &'static Children,
        ),
        (Without<RemoteBall>, Without<PlayerCamera>),
    >,
    pub bar_sprites: Query<
        'w,
        's,
        &'static mut Sprite,
        Or<(With<KickChargeBarCurveLeft>, With<KickChargeBarCurveRight>)>,
    >,
    pub player_materials: Query<
        'w,
        's,
        (
            &'static PlayerSprite,
            &'static MeshMaterial2d<ColorMaterial>,
        ),
    >,
    pub children_query: Query<'w, 's, &'static Children>,
    pub text_color_query: Query<'w, 's, &'static mut TextColor>,
}

fn process_network_messages(mut params: NetworkParams, mut queries: NetworkQueries) {
    // --- Re-asignación para mantener compatibilidad con tu lógica actual ---
    let commands = &mut params.commands;
    let embedded_assets = &params.embedded_assets;
    let config = &mut params.config;
    let channels = &params.channels;
    let my_id = &mut params.my_id;
    let loaded_map = &mut params.loaded_map;
    let mut meshes = &mut params.meshes;
    let mut materials = &mut params.materials;
    let keybindings = &params.keybindings;
    let local_players = &mut params.local_players;
    let game_tick = &mut params.game_tick;
    let gamepad_bindings_map = params.gamepad_bindings_map;
    let player_colors = &mut params.player_colors;

    let ball_q = &mut queries.ball_q;
    let players_q = &mut queries.players_q;
    let bar_sprites = &mut queries.bar_sprites;
    let player_materials = &queries.player_materials;
    let children_query = &queries.children_query;
    let text_color_query = &mut queries.text_color_query;

    let Some(ref receiver) = channels.receiver else {
        return;
    };
    let rx = receiver.lock().unwrap();
    let mut spawned_this_frame = std::collections::HashSet::new();

    // Procesar solo el último GameState si hay múltiples (incluye tick)
    let mut last_game_state: Option<(
        u32, // tick
        Vec<shared::protocol::PlayerState>,
        shared::protocol::BallState,
    )> = None;
    let mut messages = Vec::new();

    while let Ok(msg) = rx.try_recv() {
        messages.push(msg);
    }

    for msg in messages {
        match msg {
            ServerMessage::Welcome { player_id, map } => {
                println!("🎉 [Bevy] Welcome recibido. PlayerID: {}", player_id);

                // Asociar este player_id con el siguiente jugador local sin ID asignado
                if !local_players.is_empty() {
                    // Buscar el primer jugador local sin server_player_id asignado
                    if let Some(local_player) = local_players
                        .players
                        .iter_mut()
                        .find(|p| p.server_player_id.is_none())
                    {
                        local_player.server_player_id = Some(player_id);
                        println!(
                            "   Asociado a jugador local '{}' (índice {})",
                            local_player.name, local_player.local_index
                        );
                    }
                }

                // my_id guarda el primer player_id (para compatibilidad y cámara)
                if my_id.0.is_none() {
                    my_id.0 = Some(player_id);
                }

                // Almacenar mapa si fue enviado (solo del primer Welcome)
                if loaded_map.0.is_none() {
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
                last_game_state = Some((tick, players, ball));
            }
            ServerMessage::ChangeTeamColor { team_index, color } => {
                println!(
                    "🎨 Cambio de color equipo {}: ({:.2}, {:.2}, {:.2})",
                    team_index, color.0, color.1, color.2
                );

                // 1. Actualizar config
                while config.team_colors.len() <= team_index as usize {
                    config.team_colors.push((0.5, 0.5, 0.5));
                }
                config.team_colors[team_index as usize] = color;

                // 2. Calcular nuevos colores
                let (player_color, opposite_color) =
                    get_team_colors(team_index, &config.team_colors);

                // 3. Actualizar jugadores de ese equipo
                for (_, _, _, player, children) in players_q.iter() {
                    if player.team_index != team_index {
                        continue;
                    }

                    for child in children.iter() {
                        // Actualizar sprite del jugador
                        if let Ok((_, mat_handle)) = player_materials.get(child) {
                            if let Some(mat) = materials.get_mut(&mat_handle.0) {
                                mat.color = player_color;
                            }
                        }

                        // Actualizar barras de carga
                        if let Ok(mut sprite) = bar_sprites.get_mut(child) {
                            sprite.color = opposite_color;

                            // Actualizar texto hijo de la barra
                            if let Ok(bar_children) = children_query.get(child) {
                                for text_entity in bar_children.iter() {
                                    if let Ok(mut text_color) =
                                        text_color_query.get_mut(text_entity)
                                    {
                                        text_color.0 = opposite_color;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ServerMessage::PlayerDisconnected { player_id } => {
                // Buscar y eliminar el jugador desconectado
                for (entity, _, _, rp, _) in players_q.iter() {
                    if rp.id == player_id {
                        commands.entity(entity).despawn();
                        println!("👋 [Bevy] Jugador {} eliminado del juego", player_id);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    // Procesar solo el último GameState si existe
    if let Some((tick, players, ball)) = last_game_state {
        game_tick.0 = tick;

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
            // Usar textura con children
            commands
                .spawn((
                    Transform::from_xyz(ball.position.0, ball.position.1, 10.0), // Z=10 para estar sobre las líneas del mapa
                    Visibility::default(),
                    RemoteBall,
                    Collider::ball(config.ball_radius), // Para debug rendering
                    Interpolated {
                        target_position: Vec2::new(ball.position.0, ball.position.1),
                        target_velocity: Vec2::new(ball.velocity.0, ball.velocity.1),
                        target_rotation: 0.0,
                        smoothing: 20.0,
                    },
                    RenderLayers::layer(0),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Sprite {
                            image: embedded_assets.ball_texture.clone(),
                            custom_size: Some(Vec2::splat(config.ball_radius * 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(0.0, 0.0, 1.0),
                        RenderLayers::from_layers(&[0, 2]),
                    ));
                });
        }

        // Actualizar Jugadores
        for ps in players {
            let mut found = false;
            for (_entity, mut interp, mut transform, mut rp, _children) in players_q.iter_mut() {
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
                    rp.active_movement = ps.active_movement.clone();
                    rp.mode_cube_active = ps.mode_cube_active;

                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);

                // Determinar si es un jugador local (cualquiera de los jugadores locales)
                let is_local = local_players
                    .players
                    .iter()
                    .any(|lp| lp.server_player_id == Some(ps.id));

                let public_player_layers = if is_local {
                    RenderLayers::from_layers(&[0, 2]) // Visible en cámara principal y detalle
                } else {
                    RenderLayers::layer(0) // Solo visible en cámara principal
                };

                let private_player_layers = if is_local {
                    RenderLayers::from_layers(&[0, 2]) // Visible en cámara principal y detalle
                } else {
                    RenderLayers::none() // No visible
                };

                let preview_player_layers = if is_local {
                    RenderLayers::layer(2) // Visible en cámara de detalle
                } else {
                    RenderLayers::none() // No visible
                };

                println!(
                    "🆕 [Bevy] Spawneando jugador visual: {} (ID: {}) {}",
                    ps.name,
                    ps.id,
                    if is_local { "(LOCAL)" } else { "" }
                );

                // Colores de equipo desde la configuración
                let (player_color, opposite_color) =
                    get_team_colors(ps.team_index, &config.team_colors);

                // Generar color único para el jugador (para nombre y minimapa)
                let unique_player_color =
                    player_colors
                        .colors
                        .get(&ps.id)
                        .copied()
                        .unwrap_or_else(|| {
                            let color = generate_unique_player_color(player_colors);
                            player_colors.colors.insert(ps.id, color);
                            color
                        });

                // Usar textura con children
                commands
                    .spawn((
                        Transform::from_xyz(ps.position.x, ps.position.y, 10.0), // Z=10 para estar sobre las líneas del mapa
                        Visibility::default(),
                        RemotePlayer {
                            id: ps.id,
                            name: ps.name.clone(),
                            team_index: ps.team_index,
                            kick_charge: ps.kick_charge,
                            is_sliding: ps.is_sliding,
                            not_interacting: ps.not_interacting,
                            base_color: player_color,
                            ball_target_position: ps.ball_target_position,
                            stamin_charge: ps.stamin_charge,
                            active_movement: ps.active_movement.clone(),
                            mode_cube_active: ps.mode_cube_active,
                        },
                        Collider::ball(config.sphere_radius), // Para debug rendering
                        Interpolated {
                            target_position: ps.position,
                            target_velocity: Vec2::new(ps.velocity.0, ps.velocity.1),
                            target_rotation: ps.rotation,
                            smoothing: 15.0,
                        },
                        public_player_layers.clone(),
                    ))
                    .with_children(|parent| {
                        let radius = config.sphere_radius;
                        let outline_thickness = 3.0;

                        // Círculo de borde (outline) - negro, ligeramente más grande
                        parent.spawn((
                            Mesh2d(meshes.add(Circle::new(radius + outline_thickness))),
                            MeshMaterial2d(materials.add(Color::BLACK)),
                            Transform::from_xyz(0.0, 0.0, 0.5),
                            PlayerOutline,
                            public_player_layers.clone(),
                        ));

                        // Círculo principal (relleno) - color del jugador
                        parent.spawn((
                            Mesh2d(meshes.add(Circle::new(radius))),
                            MeshMaterial2d(materials.add(player_color)),
                            Transform::from_xyz(0.0, 0.0, 1.0),
                            PlayerSprite { parent_id: ps.id },
                            public_player_layers.clone(),
                        ));

                        // Indicador de dirección (cubo pequeño hacia adelante)
                        let indicator_size = radius / 3.0;
                        // Posición fija hacia adelante del jugador
                        let indicator_x = radius * 0.7;
                        let indicator_y = 0.0;
                        let cube_scale = 1.0; // Escala inicial, modificable por movimientos

                        // Mesh personalizado: cuadrado con 4 vértices (LineStrip)
                        let half = indicator_size / 2.0;
                        let mut square_mesh = Mesh::new(
                            bevy::mesh::PrimitiveTopology::LineStrip,
                            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
                        );
                        square_mesh.insert_attribute(
                            Mesh::ATTRIBUTE_POSITION,
                            vec![
                                [-half, -half, 0.0],
                                [half, -half, 0.0],
                                [half, half, 0.0],
                                [-half, half, 0.0],
                                [-half, -half, 0.0],
                            ],
                        );

                        parent.spawn((
                            Mesh2d(meshes.add(square_mesh)),
                            MeshMaterial2d(materials.add(Color::WHITE)),
                            Transform::from_xyz(indicator_x, indicator_y, 1.5)
                                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
                                .with_scale(Vec3::splat(cube_scale)),
                            SlideCubeVisual { parent_id: ps.id },
                            preview_player_layers.clone(),
                        ));

                        // Barra de carga de patada
                        parent.spawn((
                            KickChargeBar,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(0.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform::from_xyz(0.0, 0.0, 30.0),
                            private_player_layers.clone(),
                        ));

                        let angle = 25.0f32.to_radians();

                        // Barra de carga de patada a la izquierda
                        parent.spawn((
                            KickChargeBarCurveLeft,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(5.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(0.0, 10.0, 30.0),
                                // Rotación hacia la izquierda (positiva en el eje Z)
                                rotation: Quat::from_rotation_z(angle),
                                ..default()
                            },
                            private_player_layers.clone(),
                        ));

                        // Barra de carga de patada a la derecha
                        parent.spawn((
                            KickChargeBarCurveRight,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(5.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(0.0, -10.0, 30.0),
                                // Rotación hacia la derecha (negativa en el eje Z)
                                rotation: Quat::from_rotation_z(-angle),
                                ..default()
                            },
                            private_player_layers.clone(),
                        ));

                        let angle_90 = 90.0f32.to_radians();

                        // Solo para jugador local: barra de estamina
                        parent.spawn((
                            StaminChargeBar,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(0.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(-10.0, -15.0, 30.0),
                                rotation: Quat::from_rotation_z(angle_90),
                                ..default()
                            },
                            preview_player_layers,
                        ));

                        // Nombre del jugador debajo del sprite
                        parent.spawn((
                            PlayerNameText,
                            Text2d::new(ps.name.clone()),
                            TextFont {
                                font_size: 20.0,
                                ..default()
                            },
                            TextColor(unique_player_color),
                            Transform::from_xyz(-config.sphere_radius * 1.5, 0.0, 10.0),
                            RenderLayers::layer(0),
                        ));

                        // Indicadores de teclas/botones de curva (solo para jugador local)
                        // Buscar el jugador local correspondiente para saber qué tipo de input usa
                        let local_player_opt = local_players
                            .players
                            .iter()
                            .find(|lp| lp.server_player_id == Some(ps.id));

                        let (curve_left_text, curve_right_text) =
                            if let Some(local_player) = local_player_opt {
                                match &local_player.input_device {
                                    InputDevice::RawGamepad(_) => {
                                        // Usar bindings del gamepad
                                        let gamepad_bindings = local_player
                                            .gamepad_type_name
                                            .as_ref()
                                            .map(|name| gamepad_bindings_map.get_bindings(name))
                                            .unwrap_or_default();

                                        let left = gamepad_bindings
                                            .curve_left
                                            .map(|b| b.display_name())
                                            .unwrap_or_else(|| "?".to_string());
                                        let right = gamepad_bindings
                                            .curve_right
                                            .map(|b| b.display_name())
                                            .unwrap_or_else(|| "?".to_string());
                                        (left, right)
                                    }
                                    _ => {
                                        // Usar bindings del teclado
                                        (
                                            key_code_display_name(keybindings.curve_left.0),
                                            key_code_display_name(keybindings.curve_right.0),
                                        )
                                    }
                                }
                            } else {
                                // Fallback a teclado
                                (
                                    key_code_display_name(keybindings.curve_left.0),
                                    key_code_display_name(keybindings.curve_right.0),
                                )
                            };

                        let angle_90 = std::f32::consts::FRAC_PI_2;

                        // Tecla/botón izquierda (curve_left)
                        spawn_key_visual_2d(
                            parent,
                            &curve_left_text,
                            ps.id,
                            CurveAction::Left,
                            Vec3::new(
                                config.sphere_radius / 2.0,
                                -config.sphere_radius * 2.0,
                                50.0,
                            ),
                            Quat::from_rotation_z(-angle_90),
                            &mut meshes,
                            &mut materials,
                            private_player_layers.clone(),
                        );

                        // Tecla/botón derecha (curve_right)
                        spawn_key_visual_2d(
                            parent,
                            &curve_right_text,
                            ps.id,
                            CurveAction::Right,
                            Vec3::new(config.sphere_radius / 2.0, config.sphere_radius * 2.0, 50.0),
                            Quat::from_rotation_z(-angle_90),
                            &mut meshes,
                            &mut materials,
                            private_player_layers.clone(),
                        );
                    });
            }
        }
    }
}

// Sistema para mantener el nombre del jugador siempre horizontal (sin rotar)
fn keep_name_horizontal(
    mut name_query: Query<(&mut Transform, &ChildOf), With<PlayerNameText>>,
    parent_query: Query<&Transform, (With<RemotePlayer>, Without<PlayerNameText>)>,
) {
    for (mut name_transform, child_of) in name_query.iter_mut() {
        if let Ok(parent_transform) = parent_query.get(child_of.parent()) {
            // Contrarrestar la rotación del padre para que el texto quede horizontal
            name_transform.rotation = parent_transform.rotation.inverse();
        }
    }
}

// 3. Sistema de interpolación (Actualizado)
fn interpolate_entities(time: Res<Time>, mut q: Query<(&mut Transform, &Interpolated)>) {
    let dt = time.delta_secs();
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

fn camera_follow_player_and_ball(
    my_player_id: Res<MyPlayerId>,
    local_players: Res<LocalPlayers>,
    split_state: Res<DynamicSplitState>,
    _ball_query: Query<
        &Transform,
        (
            With<RemoteBall>,
            Without<PlayerCamera>,
            Without<MinimapCamera>,
            Without<PlayerDetailCamera>,
        ),
    >,
    players: Query<
        (&RemotePlayer, &Transform),
        (
            Without<PlayerCamera>,
            Without<MinimapCamera>,
            Without<PlayerDetailCamera>,
        ),
    >,
    mut cameras: ParamSet<(
        Query<(&mut Transform, &PlayerCamera)>,
        Query<&mut Transform, With<MinimapCamera>>,
        Query<(&mut Transform, &PlayerDetailCamera)>,
    )>,
    time: Res<Time>,
    windows: Query<&Window>,
) {
    let Some(_) = windows.iter().next() else {
        return;
    };

    let delta = time.delta_secs();
    let smoothing = 10.0;

    // Calcular cuántos jugadores locales hay
    let num_local_players = local_players.players.len().max(1);
    let _use_split = num_local_players > 1;

    // Recopilar posiciones de todos los jugadores locales para calcular centroide
    let mut local_player_positions: Vec<Vec3> = Vec::new();
    for local_player in local_players.players.iter().take(2) {
        if let Some(server_id) = local_player.server_player_id {
            if let Some((_, transform)) = players.iter().find(|(p, _)| p.id == server_id) {
                local_player_positions.push(transform.translation);
            }
        }
    }

    // Calcular centroide de todos los jugadores locales
    let centroid = if local_player_positions.is_empty() {
        None
    } else {
        let sum: Vec3 = local_player_positions.iter().copied().sum();
        Some(sum / local_player_positions.len() as f32)
    };

    // Factor de split: 0 = unified (seguir centroide), 1 = split (cada cámara sigue su jugador)
    let split_factor = split_state.split_factor;
    let split_angle = split_state.split_angle;

    // Calcular el vector normal del split (dirección entre jugadores)
    // Este es el eje perpendicular a la línea de división
    let split_normal = Vec2::new(split_angle.cos(), split_angle.sin());

    // Iterar sobre todas las cámaras de jugador
    for (mut cam_transform, player_camera) in cameras.p0().iter_mut() {
        // Determinar qué jugador debe seguir esta cámara
        let target_player_id = player_camera.server_player_id.or(my_player_id.0);

        let Some(target_id) = target_player_id else {
            continue;
        };

        // Buscar la posición del jugador objetivo
        let player_pos = players
            .iter()
            .find(|(p, _)| p.id == target_id)
            .map(|(_, t)| t.translation);

        if let Some(p_pos) = player_pos {
            // Centroide de los jugadores
            let unified_target = centroid.unwrap_or(p_pos);

            // Cuando split_factor > 0, calcular el centro de la región de esta cámara
            //
            // La pantalla se divide con una línea. Cada mitad es un cuadrilátero.
            // El centro del cuadrilátero es la intersección de sus diagonales.
            //
            // Para simplificar: el centro de cada mitad está aproximadamente a 1/4
            // del viewport desde el centro, EN LA DIRECCIÓN del jugador respecto al centro.
            //
            // En vez de usar el split_normal (que apunta entre jugadores),
            // calculamos el offset basado en la posición del jugador respecto al centroide.

            let split_target = if let Some(cent) = centroid {
                // Vector desde este jugador hacia el centroide (hacia el OTRO jugador)
                let to_center = cent - p_pos;
                let distance_to_center = to_center.truncate().length();

                if distance_to_center > 1.0 {
                    // Para que el jugador aparezca en el centro de SU mitad de pantalla:
                    // - El centro de su mitad está DESPLAZADO del centro de pantalla
                    // - El desplazamiento es HACIA su lado (alejándose del otro jugador)
                    // - Para lograr esto, la cámara debe moverse HACIA el otro jugador
                    //
                    // Magnitud: ~1/4 del viewport visible
                    let visible_quarter = 240.0 * 3.0; // ~720 unidades a scale 3.0

                    // Dirección: hacia el centroide (hacia el otro jugador)
                    let dir = to_center.truncate().normalize();

                    // Mover cámara HACIA el otro jugador
                    // Esto hace que el jugador aparezca desplazado hacia SU lado de la pantalla
                    let camera_offset =
                        Vec3::new(dir.x * visible_quarter, dir.y * visible_quarter, 0.0);

                    p_pos + camera_offset
                } else {
                    p_pos
                }
            } else {
                p_pos
            };

            // Interpolar entre unified (centroide) y split (jugador en centro de su región)
            let final_target = unified_target.lerp(split_target, split_factor);

            // Aplicar movimiento suavizado
            cam_transform.translation.x +=
                (final_target.x - cam_transform.translation.x) * smoothing * delta;
            cam_transform.translation.y +=
                (final_target.y - cam_transform.translation.y) * smoothing * delta;
        }
    }

    // 5. Cámaras de Detalle (cada una sigue a su jugador local correspondiente)
    for (mut cam_transform, detail_camera) in cameras.p2().iter_mut() {
        // Buscar el jugador local correspondiente a esta cámara de detalle
        let target_player_id = local_players
            .players
            .get(detail_camera.local_index as usize)
            .and_then(|lp| lp.server_player_id)
            .or_else(|| {
                // Fallback: si no hay jugador local, usar my_player_id para la primera cámara
                if detail_camera.local_index == 0 {
                    my_player_id.0
                } else {
                    None
                }
            });

        if let Some(target_id) = target_player_id {
            if let Some(p_pos) = players
                .iter()
                .find(|(p, _)| p.id == target_id)
                .map(|(_, t)| t.translation)
            {
                cam_transform.translation.x = p_pos.x;
                cam_transform.translation.y = p_pos.y;
            }
        }
    }
}

// Sistema de control de zoom con teclas numéricas
fn camera_zoom_control(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut cameras: Query<&mut Projection, With<PlayerCamera>>,
) {
    let mut new_scale = None;

    // Teclas 1-9 para diferentes niveles de zoom
    if keyboard.just_pressed(KeyCode::Digit9) {
        new_scale = Some(0.5); // Muy cerca
    } else if keyboard.just_pressed(KeyCode::Digit8) {
        new_scale = Some(0.75);
    } else if keyboard.just_pressed(KeyCode::Digit7) {
        new_scale = Some(1.0); // Normal
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        new_scale = Some(1.3);
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        new_scale = Some(1.5);
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        new_scale = Some(2.0); // Lejos
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        new_scale = Some(2.5);
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        new_scale = Some(3.0);
    } else if keyboard.just_pressed(KeyCode::Digit1) {
        new_scale = Some(4.0); // Muy lejos
    }

    if let Some(scale) = new_scale {
        // Aplicar zoom a todas las cámaras de jugador
        for mut projection_comp in cameras.iter_mut() {
            if let Projection::Orthographic(ref mut projection) = projection_comp.as_mut() {
                projection.scale = scale;
            }
        }
        println!("📷 Zoom ajustado a: {:.1}x", scale);
    }
}

/// Sistema que sincroniza los server_player_id de las cámaras con LocalPlayers
/// (Ya no maneja viewports porque usamos render-to-texture para split dinámico)
fn update_camera_viewports(
    local_players: Res<LocalPlayers>,
    mut cameras: Query<&mut PlayerCamera>,
) {
    for mut player_cam in cameras.iter_mut() {
        // Sincronizar server_player_id desde LocalPlayers
        if let Some(local_player) = local_players.players.get(player_cam.local_index as usize) {
            player_cam.server_player_id = local_player.server_player_id;
        }
    }
}

/// Calcula el ángulo del vector entre jugadores.
/// El shader usa este ángulo como normal - la línea de corte será perpendicular.
/// Nota: Las coordenadas UV tienen Y invertido, por eso negamos direction.y
fn calculate_split_angle(player1_pos: Vec2, player2_pos: Vec2) -> f32 {
    let direction = player2_pos - player1_pos;
    // El ángulo es la dirección entre jugadores (el shader dibuja la línea perpendicular)
    // Negamos Y porque las UV tienen Y invertido respecto al mundo
    (-direction.y).atan2(direction.x)
}

/// Sistema que actualiza el estado del split-screen dinámico
fn update_split_screen_state(
    local_players: Res<LocalPlayers>,
    players: Query<(&RemotePlayer, &Transform)>,
    cameras: Query<&Projection, With<PlayerCamera>>,
    mut split_state: ResMut<DynamicSplitState>,
    time: Res<Time>,
) {
    // Solo procesar si hay 2+ jugadores locales
    if local_players.players.len() < 2 {
        split_state.mode = SplitMode::Unified;
        split_state.split_factor = 0.0;
        return;
    }

    // Obtener posiciones de los jugadores locales
    let mut local_positions: Vec<Vec2> = Vec::new();

    for local_player in local_players.players.iter().take(2) {
        if let Some(server_id) = local_player.server_player_id {
            if let Some((_, transform)) = players.iter().find(|(p, _)| p.id == server_id) {
                local_positions.push(transform.translation.truncate());
            }
        }
    }

    // Necesitamos al menos 2 posiciones
    if local_positions.len() < 2 {
        return;
    }

    let pos1 = local_positions[0];
    let pos2 = local_positions[1];

    // Calcular distancia entre jugadores
    let distance = pos1.distance(pos2);

    // Obtener escala de zoom para ajustar umbrales
    let zoom_scale = cameras
        .iter()
        .next()
        .and_then(|p| {
            if let Projection::Orthographic(ortho) = p {
                Some(ortho.scale)
            } else {
                None
            }
        })
        .unwrap_or(3.0);

    // Calcular umbral visible basado en zoom (aproximación del viewport visible)
    // A mayor zoom (scale), menos se ve, así que los umbrales deben ser menores
    let base_visible = 600.0; // Distancia base visible a zoom 1.0
    let visible_distance = base_visible * zoom_scale;

    // Umbrales con histéresis para evitar parpadeo
    let split_threshold = visible_distance * 0.8; // 80% del visible para separar
    let merge_threshold = visible_distance * 0.5; // 50% del visible para fusionar

    split_state.split_threshold = split_threshold;
    split_state.merge_threshold = merge_threshold;
    split_state.viewport_visible_ratio = zoom_scale;

    // Determinar modo objetivo
    let target_mode = match split_state.mode {
        SplitMode::Unified => {
            if distance > split_threshold {
                SplitMode::Split
            } else {
                SplitMode::Unified
            }
        }
        SplitMode::Split => {
            if distance < merge_threshold {
                SplitMode::Unified
            } else {
                SplitMode::Split
            }
        }
        SplitMode::Transitioning => {
            // Durante transición, usar umbrales sin histéresis
            if distance > (split_threshold + merge_threshold) / 2.0 {
                SplitMode::Split
            } else {
                SplitMode::Unified
            }
        }
    };

    // Calcular factor objetivo
    let target_factor = match target_mode {
        SplitMode::Unified => 0.0,
        SplitMode::Split => 1.0,
        SplitMode::Transitioning => split_state.split_factor,
    };

    // Interpolación suave del factor
    let transition_speed = 5.0;
    let t = time.delta_secs() * transition_speed;
    let new_factor = split_state.split_factor + (target_factor - split_state.split_factor) * t;

    // Actualizar modo basado en el factor
    split_state.mode = if new_factor < 0.01 {
        SplitMode::Unified
    } else if new_factor > 0.99 {
        SplitMode::Split
    } else {
        SplitMode::Transitioning
    };

    split_state.split_factor = new_factor.clamp(0.0, 1.0);

    // Calcular ángulo de división
    split_state.split_angle = calculate_split_angle(pos1, pos2);
}

/// Sistema que actualiza el material del compositor con los parámetros actuales
fn update_split_compositor(
    split_state: Res<DynamicSplitState>,
    split_quad: Query<&MeshMaterial2d<SplitScreenMaterial>, With<SplitScreenQuad>>,
    mut split_materials: ResMut<Assets<SplitScreenMaterial>>,
) {
    for material_handle in split_quad.iter() {
        if let Some(material) = split_materials.get_mut(material_handle) {
            // Actualizar los parámetros del shader
            // x: angle, y: factor, z: center_x, w: center_y
            material.split_params = Vec4::new(
                split_state.split_angle,
                split_state.split_factor,
                0.5, // Centro X (normalizado 0-1)
                0.5, // Centro Y (normalizado 0-1)
            );
        }
    }
}

fn update_charge_bar(
    player_query: Query<(&RemotePlayer, &Children)>,
    config: Res<GameConfig>,
    mut sprite_query: Query<&mut Sprite>,
    mut mesh_query: Query<&mut Mesh2d>,
    // Necesitamos acceso mutable a los materiales para cambiar el color
    mut materials: ResMut<Assets<ColorMaterial>>,
    material_query: Query<&MeshMaterial2d<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    bar_main_q: Query<Entity, With<KickChargeBar>>,
    bar_left_q: Query<Entity, With<KickChargeBarCurveLeft>>,
    bar_right_q: Query<Entity, With<KickChargeBarCurveRight>>,
    player_outline_q: Query<Entity, With<PlayerOutline>>,
) {
    let max_width = 45.0;
    let radius = config.sphere_radius;
    let outline_thickness = 3.0;
    let max_outline_thickness = 7.0;

    for (player, children) in player_query.iter() {
        let charge_pct = player.kick_charge.x; // De 0.0 a 1.0

        for child in children.iter() {
            // --- Lógica de Sprites (Barras) ---
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct, 5.0));
                } else if bar_left_q.contains(child) {
                    let coef = if player.kick_charge.y < 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct * coef, 5.0));
                } else if bar_right_q.contains(child) {
                    let coef = if player.kick_charge.y > 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct * coef, 5.0));
                }
            }
            // --- Lógica del Outline (Mesh + Color) ---
            else if player_outline_q.contains(child) {
                // 1. Actualizar el tamaño del Mesh
                if let Ok(mut mesh_handle) = mesh_query.get_mut(child) {
                    let dynamic_thickness = charge_pct * max_outline_thickness;
                    let new_radius = radius + outline_thickness + dynamic_thickness;
                    *mesh_handle = meshes.add(Circle::new(new_radius)).into();
                }

                // 2. Actualizar el Color (de Negro a Blanco)
                if let Ok(mat_handle) = material_query.get(child) {
                    if let Some(material) = materials.get_mut(mat_handle) {
                        let r = charge_pct; // De 0.0 a 1.0
                        let g = charge_pct;
                        let b = charge_pct;
                        material.color = Color::LinearRgba(LinearRgba::new(r, g, b, 1.0));
                    }
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
    bar_main_q: Query<Entity, With<StaminChargeBar>>,
) {
    let max_width = 30.0;

    for (player, children) in player_query.iter() {
        for child in children.iter() {
            // Intentamos obtener el sprite del hijo
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                // 1. Caso: Barra Principal
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * player.stamin_charge, 5.0));
                }
            }
        }
    }
}

fn update_player_sprite(
    player_query: Query<&RemotePlayer>,
    sprite_query: Query<(&PlayerSprite, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (player_sprite, material_handle) in sprite_query.iter() {
        // Buscamos al jugador padre para obtener su color base y estado
        if let Some(player) = player_query
            .iter()
            .find(|p| p.id == player_sprite.parent_id)
        {
            // Aplicar color y transparencia al material
            let alpha = if player.not_interacting { 0.3 } else { 1.0 };

            if let Some(material) = materials.get_mut(&material_handle.0) {
                material.color = player.base_color.with_alpha(alpha);
            }
        }
    }
}

// Sistema para procesar movimientos activos y actualizar el cubo de dirección
fn process_movements(
    game_tick: Res<GameTick>,
    player_query: Query<(&RemotePlayer, &Children)>,
    mut cube_query: Query<(&SlideCubeVisual, &mut Transform)>,
    config: Res<GameConfig>,
) {
    let current_tick = game_tick.0;

    for (player, children) in player_query.iter() {
        // Obtener el movimiento activo del jugador (si existe)
        let Some(ref active_movement) = player.active_movement else {
            continue;
        };

        // Calcular progreso basado en ticks
        let start = active_movement.start_tick;
        let end = active_movement.end_tick;

        // Si ya pasó el end_tick, el movimiento terminó
        if current_tick >= end {
            continue;
        }

        // Si aún no llegamos al start_tick, no ejecutar
        if current_tick < start {
            continue;
        }

        // Calcular progreso (0.0 a 1.0)
        let duration = (end - start) as f32;
        let elapsed = (current_tick - start) as f32;
        let progress = (elapsed / duration).clamp(0.0, 1.0);

        // Obtener el movimiento desde el catálogo compartido
        let Some(movement) = get_movement(active_movement.movement_id) else {
            continue;
        };

        // Buscar el cubo hijo de este jugador
        for child in children.iter() {
            if let Ok((cube_visual, mut cube_transform)) = cube_query.get_mut(child) {
                if cube_visual.parent_id != player.id {
                    continue;
                }

                // Evaluar cada propiedad animada usando keyframes
                // Scale
                if let Some(scale) = movement.evaluate(AnimatedProperty::Scale, progress) {
                    cube_transform.scale = Vec3::splat(scale);
                }

                // OffsetX (multiplicador del radio)
                if let Some(offset_mult) = movement.evaluate(AnimatedProperty::OffsetX, progress) {
                    cube_transform.translation.x = config.sphere_radius * offset_mult;
                }

                // OffsetY (multiplicador del radio)
                if let Some(offset_mult) = movement.evaluate(AnimatedProperty::OffsetY, progress) {
                    cube_transform.translation.y = config.sphere_radius * offset_mult;
                }

                // Rotación adicional (se suma a la base de 45°)
                if let Some(rotation) = movement.evaluate(AnimatedProperty::Rotation, progress) {
                    cube_transform.rotation =
                        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4 + rotation);
                }
            }
        }
    }
}

// Sistema para actualizar visualización del cubo según modo activo
fn update_mode_visuals(
    player_query: Query<(&RemotePlayer, &Children)>,
    mut cube_query: Query<(&SlideCubeVisual, &mut Transform)>,
    mut sphere_query: Query<(&PlayerSprite, &mut Transform), Without<SlideCubeVisual>>,
    config: Res<GameConfig>,
) {
    for (player, children) in player_query.iter() {
        // Buscar el cubo y la esfera hijos de este jugador
        for child in children.iter() {
            // Actualizar cubo
            if let Ok((cube_visual, mut cube_transform)) = cube_query.get_mut(child) {
                if cube_visual.parent_id != player.id {
                    continue;
                }

                // Si hay un movimiento activo, no sobreescribir (el sistema de movimientos tiene prioridad)
                if player.active_movement.is_some() && player.is_sliding {
                    continue;
                }

                if player.mode_cube_active {
                    // Modo cubo: grande y centrado
                    cube_transform.scale = Vec3::splat(2.5);
                    cube_transform.translation.x = 0.0;
                    cube_transform.translation.y = 0.0;
                } else {
                    // Modo normal: pequeño y en offset
                    cube_transform.scale = Vec3::splat(1.0);
                    cube_transform.translation.x = config.sphere_radius * 0.7;
                    cube_transform.translation.y = 0.0;
                }
            }

            // Actualizar esfera (escala)
            if let Ok((sprite, mut sprite_transform)) = sphere_query.get_mut(child) {
                if sprite.parent_id != player.id {
                    continue;
                }

                if player.mode_cube_active {
                    // Modo cubo: esfera chica
                    sprite_transform.scale = Vec3::splat(0.3);
                } else {
                    // Modo normal: esfera tamaño normal
                    sprite_transform.scale = Vec3::splat(1.0);
                }
            }
        }
    }
}

// Sistema para ocultar líneas por defecto, ajustar campo y crear líneas del mapa
fn adjust_field_for_map(
    mut commands: Commands,
    loaded_map: Res<LoadedMap>,
    mut default_lines: Query<&mut Visibility, With<DefaultFieldLine>>,
    mut field_bg: Query<
        (&mut Sprite, &mut Transform),
        (
            With<FieldBackground>,
            Without<DefaultFieldLine>,
            Without<MinimapFieldBackground>,
        ),
    >,
    mut minimap_bg: Query<&mut Sprite, (With<MinimapFieldBackground>, Without<FieldBackground>)>,
    mut minimap_camera: Query<&mut Projection, With<MinimapCamera>>,
    map_lines: Query<Entity, With<MapLineEntity>>,
    minimap_lines: Query<Entity, With<MinimapFieldLine>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if loaded_map.is_changed() {
        // Eliminar líneas del mapa anterior
        for entity in map_lines.iter() {
            commands.entity(entity).despawn();
        }
        // Eliminar líneas del minimapa anterior
        for entity in minimap_lines.iter() {
            commands.entity(entity).despawn();
        }

        if let Some(map) = &loaded_map.0 {
            // Hay mapa: ocultar líneas por defecto
            for mut visibility in default_lines.iter_mut() {
                *visibility = Visibility::Hidden;
            }

            // Ajustar tamaño del campo según dimensiones del mapa
            let width = map.width.or(map.bg.width);
            let height = map.height.or(map.bg.height);

            if let (Some(w), Some(h)) = (width, height) {
                // Campo principal
                if let Ok((mut sprite, _transform)) = field_bg.single_mut() {
                    sprite.custom_size = Some(Vec2::new(w, h));
                    println!("🎨 Campo ajustado a dimensiones del mapa: {}x{}", w, h);
                }
                // Fondo del minimapa
                if let Ok(mut minimap_sprite) = minimap_bg.single_mut() {
                    minimap_sprite.custom_size = Some(Vec2::new(w, h));
                }
                // Proyección de la cámara del minimapa
                // Ajustar para que el mapa llene el minimapa (300x180), 2x más cerca
                if let Ok(mut projection) = minimap_camera.single_mut() {
                    let minimap_aspect = 300.0 / 180.0; // aspect ratio del minimapa
                    let map_aspect = w / h;
                    let zoom = 0.5; // 2x más cerca

                    let (cam_w, cam_h) = if map_aspect > minimap_aspect {
                        // Mapa más ancho: el ancho define la escala
                        (w * zoom, w / minimap_aspect * zoom)
                    } else {
                        // Mapa más alto: la altura define la escala
                        (h * minimap_aspect * zoom, h * zoom)
                    };

                    *projection = Projection::Orthographic(OrthographicProjection {
                        scaling_mode: ScalingMode::Fixed {
                            width: cam_w,
                            height: cam_h,
                        },
                        ..OrthographicProjection::default_2d()
                    });
                    println!("🗺️  Cámara minimapa ajustada a: {}x{}", cam_w, cam_h);
                }
            } else {
                println!("⚠️  Mapa sin dimensiones definidas, usando tamaño por defecto");
            }

            // Crear líneas del mapa como sprites
            spawn_map_lines(&mut commands, map, &mut meshes, &mut materials);
            // Crear líneas del minimapa
            spawn_minimap_lines(&mut commands, map);
        } else {
            // No hay mapa: mostrar líneas por defecto
            for mut visibility in default_lines.iter_mut() {
                *visibility = Visibility::Visible;
            }
        }
    }
}

// Constante Z para las líneas del mapa (entre el piso Z=0 y los jugadores Z=10+)
const MAP_LINES_Z: f32 = 5.0;
const LINE_THICKNESS: f32 = 3.0;

// Crea sprites para las líneas del mapa
fn spawn_map_lines(
    commands: &mut Commands,
    map: &shared::map::Map,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    println!(
        "🗺️  spawn_map_lines: {} vértices, {} segmentos, {} discos",
        map.vertexes.len(),
        map.segments.len(),
        map.discs.len()
    );

    // Colores según tipo de interacción
    let ball_color = Color::srgb(0.3, 0.7, 1.0); // Azul claro - solo pelota
    let player_color = Color::srgb(0.3, 1.0, 0.5); // Verde claro - solo jugadores
    let decorative_color = Color::srgb(0.5, 0.5, 0.5); // Gris - decorativo sin física
    let vertex_color = Color::srgb(1.0, 0.2, 0.2); // Rojo para vértices
    let disc_color = Color::srgb(0.7, 0.7, 0.7); // Gris para discos

    // Dibujar vértices (círculos pequeños)
    for vertex in &map.vertexes {
        let pos = Vec2::new(vertex.x, vertex.y);
        spawn_circle(commands, meshes, materials, pos, 3.0, vertex_color);
    }

    // Dibujar segmentos (líneas)
    for segment in &map.segments {
        if !segment.is_visible() {
            continue;
        }

        if segment.v0 >= map.vertexes.len() || segment.v1 >= map.vertexes.len() {
            continue;
        }

        let v0 = &map.vertexes[segment.v0];
        let v1 = &map.vertexes[segment.v1];

        let p0 = Vec2::new(v0.x, v0.y);
        let p1 = Vec2::new(v1.x, v1.y);

        // Determinar color según cMask
        let line_color = if let Some(cmask) = &segment.c_mask {
            if cmask.is_empty() || cmask.iter().any(|m| m.is_empty()) {
                decorative_color
            } else if cmask.iter().any(|m| m == "ball")
                && !cmask.iter().any(|m| m == "red" || m == "blue")
            {
                ball_color
            } else if cmask.iter().any(|m| m == "red" || m == "blue") {
                player_color
            } else {
                decorative_color
            }
        } else {
            decorative_color
        };

        let curve_factor = segment.curve.or(segment.curve_f).unwrap_or(0.0);

        if curve_factor.abs() < 0.01 {
            // Segmento recto
            spawn_line_segment(commands, p0, p1, line_color);
        } else {
            // Segmento curvo - aproximar con múltiples líneas
            let points = approximate_curve_for_rendering(p0, p1, curve_factor, 24);
            for i in 0..points.len() - 1 {
                spawn_line_segment(commands, points[i], points[i + 1], line_color);
            }
        }
    }

    // Dibujar discos (círculos)
    for disc in &map.discs {
        let pos = Vec2::new(disc.pos[0], disc.pos[1]);
        spawn_circle_outline(commands, meshes, materials, pos, disc.radius, disc_color);
    }
}

// Constantes para el minimapa
const MINIMAP_LINE_Z: f32 = 0.0;

// Crea sprites para las líneas del minimapa (layer 1)
fn spawn_minimap_lines(commands: &mut Commands, map: &shared::map::Map) {
    let line_color = Color::srgba(1.0, 1.0, 1.0, 0.7);

    // Calcular grosor proporcional al tamaño del mapa
    // Para que las líneas se vean de ~3px en un minimapa de 300px
    let map_width = map.width.or(map.bg.width).unwrap_or(1000.0);
    let line_thickness = map_width / 200.0; // ~0.5% del ancho del mapa

    // Dibujar segmentos visibles
    for segment in &map.segments {
        if !segment.is_visible() {
            continue;
        }

        if segment.v0 >= map.vertexes.len() || segment.v1 >= map.vertexes.len() {
            continue;
        }

        let v0 = &map.vertexes[segment.v0];
        let v1 = &map.vertexes[segment.v1];

        let p0 = Vec2::new(v0.x, v0.y);
        let p1 = Vec2::new(v1.x, v1.y);

        let curve_factor = segment.curve.or(segment.curve_f).unwrap_or(0.0);

        if curve_factor.abs() < 0.01 {
            // Segmento recto
            spawn_minimap_line_segment(commands, p0, p1, line_color, line_thickness);
        } else {
            // Segmento curvo - aproximar con múltiples líneas
            let points = approximate_curve_for_rendering(p0, p1, curve_factor, 16);
            for i in 0..points.len() - 1 {
                spawn_minimap_line_segment(
                    commands,
                    points[i],
                    points[i + 1],
                    line_color,
                    line_thickness,
                );
            }
        }
    }
}

// Crea un sprite rectangular para el minimapa (layer 1)
fn spawn_minimap_line_segment(
    commands: &mut Commands,
    p0: Vec2,
    p1: Vec2,
    color: Color,
    thickness: f32,
) {
    let delta = p1 - p0;
    let length = delta.length();
    if length < 0.01 {
        return;
    }

    let midpoint = (p0 + p1) * 0.5;
    let angle = delta.y.atan2(delta.x);

    commands.spawn((
        Sprite {
            color,
            custom_size: Some(Vec2::new(length, thickness)),
            ..default()
        },
        Transform::from_xyz(midpoint.x, midpoint.y, MINIMAP_LINE_Z)
            .with_rotation(Quat::from_rotation_z(angle)),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
}

// Crea un sprite rectangular para representar una línea
fn spawn_line_segment(commands: &mut Commands, p0: Vec2, p1: Vec2, color: Color) {
    let delta = p1 - p0;
    let length = delta.length();
    if length < 0.01 {
        return;
    }

    let midpoint = (p0 + p1) * 0.5;
    let angle = delta.y.atan2(delta.x);

    commands.spawn((
        Sprite {
            color,
            custom_size: Some(Vec2::new(length, LINE_THICKNESS)),
            ..default()
        },
        Transform::from_xyz(midpoint.x, midpoint.y, MAP_LINES_Z)
            .with_rotation(Quat::from_rotation_z(angle)),
        MapLineEntity,
        RenderLayers::layer(0),
    ));
}

// Crea un círculo relleno
fn spawn_circle(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    pos: Vec2,
    radius: f32,
    color: Color,
) {
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(materials.add(color)),
        Transform::from_xyz(pos.x, pos.y, MAP_LINES_Z),
        MapLineEntity,
        RenderLayers::layer(0),
    ));
}

// Crea un círculo solo con borde (outline)
fn spawn_circle_outline(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    pos: Vec2,
    radius: f32,
    color: Color,
) {
    // Crear anillo usando círculo exterior menos interior
    let outline_thickness = LINE_THICKNESS;

    // Círculo exterior (borde)
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(materials.add(color)),
        Transform::from_xyz(pos.x, pos.y, MAP_LINES_Z),
        MapLineEntity,
        RenderLayers::layer(0),
    ));

    // Círculo interior (transparente/color del fondo) - simula outline
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(radius - outline_thickness))),
        MeshMaterial2d(materials.add(Color::srgba(0.0, 0.0, 0.0, 0.0))), // Transparente
        Transform::from_xyz(pos.x, pos.y, MAP_LINES_Z + 0.1),            // Ligeramente por encima
        MapLineEntity,
        RenderLayers::layer(0),
    ));
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

// ============================================================================
// SISTEMAS DE MINIMAPA
// ============================================================================

/// Crea puntos y nombres en Layer 1 cuando aparecen jugadores/pelota
fn spawn_minimap_dots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    config: Res<GameConfig>,
    player_colors: Res<PlayerColors>,
    existing_dots: Query<&MinimapDot>,
    existing_names: Query<&MinimapPlayerName>,
    players_with_dots: Query<(Entity, &RemotePlayer)>,
    ball_with_dots: Query<Entity, With<RemoteBall>>,
) {
    // Crear set de entidades ya trackeadas por dots
    let tracked_dots: std::collections::HashSet<Entity> =
        existing_dots.iter().map(|dot| dot.tracks_entity).collect();

    // Crear set de entidades ya trackeadas por nombres
    let tracked_names: std::collections::HashSet<Entity> = existing_names
        .iter()
        .map(|name| name.tracks_entity)
        .collect();

    // Spawn dots y nombres para jugadores que aún no tienen
    for (entity, player) in players_with_dots.iter() {
        // Spawn dot si no existe
        if !tracked_dots.contains(&entity) {
            // Color del equipo desde config
            let team_color = config
                .team_colors
                .get(player.team_index as usize)
                .copied()
                .unwrap_or((0.5, 0.5, 0.5));

            let dot_color = Color::srgb(team_color.0, team_color.1, team_color.2);

            // Círculo de 120px para jugadores
            commands.spawn((
                Mesh2d(meshes.add(Circle::new(120.0))),
                MeshMaterial2d(materials.add(dot_color)),
                Transform::from_xyz(0.0, 0.0, 10.0),
                MinimapDot {
                    tracks_entity: entity,
                },
                RenderLayers::layer(1),
            ));
        }

        // Spawn nombre si no existe
        if !tracked_names.contains(&entity) {
            // Usar color único del jugador para el nombre en el minimapa
            let name_color = player_colors
                .colors
                .get(&player.id)
                .copied()
                .unwrap_or(Color::WHITE);

            // Crear un nodo de texto para el nombre del jugador
            commands.spawn((
                Text2d::new(player.name.clone()),
                TextFont {
                    font_size: 80.0,
                    ..default()
                },
                TextColor(name_color),
                Transform::from_xyz(0.0, 150.0, 12.0), // Posición encima del dot
                MinimapPlayerName {
                    tracks_entity: entity,
                },
                RenderLayers::layer(1),
            ));
        }
    }

    // Spawn dot para pelota si no tiene
    for entity in ball_with_dots.iter() {
        if tracked_dots.contains(&entity) {
            continue;
        }

        // Círculo de 80px blanco para pelota
        commands.spawn((
            Mesh2d(meshes.add(Circle::new(80.0))),
            MeshMaterial2d(materials.add(Color::WHITE)),
            Transform::from_xyz(0.0, 0.0, 11.0),
            MinimapDot {
                tracks_entity: entity,
            },
            RenderLayers::layer(1),
        ));
    }
}

/// Sincroniza posición de puntos con entidades reales
fn sync_minimap_dots(
    mut dots: Query<(&MinimapDot, &mut Transform)>,
    transforms: Query<&Transform, Without<MinimapDot>>,
) {
    for (dot, mut dot_transform) in dots.iter_mut() {
        if let Ok(tracked_transform) = transforms.get(dot.tracks_entity) {
            dot_transform.translation.x = tracked_transform.translation.x;
            dot_transform.translation.y = tracked_transform.translation.y;
        }
    }
}

/// Elimina puntos cuando desaparecen entidades
fn cleanup_minimap_dots(
    mut commands: Commands,
    dots: Query<(Entity, &MinimapDot)>,
    names: Query<(Entity, &MinimapPlayerName)>,
    entities: Query<Entity>,
) {
    // Limpiar dots
    for (dot_entity, dot) in dots.iter() {
        // Si la entidad trackeada ya no existe, eliminar el dot
        if entities.get(dot.tracks_entity).is_err() {
            commands.entity(dot_entity).despawn();
        }
    }

    // Limpiar nombres
    for (name_entity, name) in names.iter() {
        if entities.get(name.tracks_entity).is_err() {
            commands.entity(name_entity).despawn();
        }
    }
}

/// Sincroniza posición de nombres del minimapa con entidades reales
fn sync_minimap_names(
    mut names: Query<(&MinimapPlayerName, &mut Transform), Without<MinimapDot>>,
    transforms: Query<&Transform, (Without<MinimapPlayerName>, Without<MinimapDot>)>,
) {
    for (name, mut name_transform) in names.iter_mut() {
        if let Ok(tracked_transform) = transforms.get(name.tracks_entity) {
            name_transform.translation.x = tracked_transform.translation.x;
            name_transform.translation.y = tracked_transform.translation.y + 150.0;
            // Encima del dot
        }
    }
}

fn animate_keys(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    key_query: Query<(&KeyVisual, &Children)>,
    mut transform_query: Query<&mut Transform>,
    time: Res<Time>,
    local_players: Res<LocalPlayers>,
    keybindings: Res<KeyBindingsConfig>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    gilrs: Option<Res<GilrsWrapper>>,
) {
    // Pre-cargar el estado de gilrs si está disponible
    let gilrs_guard = gilrs.as_ref().and_then(|g| g.gilrs.lock().ok());

    for (key_visual, children) in key_query.iter() {
        // El cuerpo móvil es el segundo hijo (índice 1) según nuestro spawn_key_visual_2d
        if let Some(&body_entity) = children.get(1) {
            if let Ok(mut transform) = transform_query.get_mut(body_entity) {
                // Buscar el jugador local correspondiente
                let local_player = local_players
                    .players
                    .iter()
                    .find(|lp| lp.server_player_id == Some(key_visual.player_id));

                // Determinar si el botón está presionado
                let is_pressed = if let Some(lp) = local_player {
                    match &lp.input_device {
                        InputDevice::Keyboard => {
                            // Usar keybindings de teclado
                            let key_code = match key_visual.action {
                                CurveAction::Left => keybindings.curve_left.0,
                                CurveAction::Right => keybindings.curve_right.0,
                            };
                            keyboard_input.pressed(key_code)
                        }
                        InputDevice::RawGamepad(gamepad_id) => {
                            // Usar bindings del gamepad
                            if let Some(ref gilrs_instance) = gilrs_guard {
                                if let Some(gamepad) = gilrs_instance.connected_gamepad(*gamepad_id)
                                {
                                    let bindings = lp
                                        .gamepad_type_name
                                        .as_ref()
                                        .map(|name| gamepad_bindings_map.get_bindings(name))
                                        .unwrap_or_default();

                                    let binding = match key_visual.action {
                                        CurveAction::Left => &bindings.curve_left,
                                        CurveAction::Right => &bindings.curve_right,
                                    };

                                    is_gamepad_binding_active(gamepad, binding)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                } else {
                    false
                };

                // Si el botón está presionado, el objetivo es -4.0 (hundida hacia la sombra)
                // Si no, el objetivo es 0.0 (posición original)
                let target_y = if is_pressed { -4.0 } else { 0.0 };

                // Usamos un lerp suave para que la tecla no "teletransporte",
                // sino que se sienta elástica y física.
                let speed = 25.0;
                transform.translation.y = transform.translation.y
                    + (target_y - transform.translation.y) * speed * time.delta_secs();
            }
        }
    }
}

/// Helper para verificar si un binding de gamepad está activo
fn is_gamepad_binding_active(
    gamepad: gilrs::Gamepad<'_>,
    binding: &Option<RawGamepadInput>,
) -> bool {
    if let Some(b) = binding {
        match b {
            RawGamepadInput::Button(idx) => {
                // Verificar si el botón está presionado
                for (code, data) in gamepad.state().buttons() {
                    if data.is_pressed() {
                        let raw_code: u32 = code.into_u32();
                        let button_idx = if raw_code >= 288 && raw_code < 320 {
                            (raw_code - 288) as u8
                        } else if raw_code >= 304 && raw_code < 320 {
                            (raw_code - 304) as u8
                        } else {
                            (raw_code & 0x1F) as u8
                        };
                        if button_idx == *idx {
                            return true;
                        }
                    }
                }
                false
            }
            RawGamepadInput::AxisPositive(idx) => {
                idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) > 0.5)
            }
            RawGamepadInput::AxisNegative(idx) => {
                idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) < -0.5)
            }
        }
    } else {
        false
    }
}
