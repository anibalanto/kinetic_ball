use bevy::asset::RenderAssetUsages;
use bevy::camera::{visibility::RenderLayers, ScalingMode};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_rapier2d::prelude::*;
use clap::Parser;
use matchbox_socket::WebRtcSocket;
use shared::movements::{get_movement, AnimatedProperty};
use shared::protocol::PlayerMovement;
use shared::protocol::{ControlMessage, GameConfig, GameDataMessage, PlayerInput, ServerMessage};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

mod keybindings;
use keybindings::{
    key_code_display_name, load_app_config, load_keybindings, save_app_config, save_keybindings,
    AppConfig, GameAction, KeyBindingsConfig, SettingsUIState,
};

mod host;
mod shared;

// ============================================================================
// ASSETS EMBEBIDOS EN EL BINARIO
// ============================================================================

const BALL_PNG: &[u8] = include_bytes!("../assets/ball.png");
const EMOJI_FONT: &[u8] = include_bytes!("../assets/NotoColorEmoji_WindowsCompatible.ttf");
const DEFAULT_MAP: &str = include_str!("../assets/cancha_grande.hbs");

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
    App::new()
        .insert_resource(bevy::winit::WinitSettings::game())
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.15)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustBall".to_string(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
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
        // Keybindings configurables
        .insert_resource(load_keybindings())
        .insert_resource(SettingsUIState::default())
        // Room selection resources
        .insert_resource(RoomList::default())
        .insert_resource(RoomFetchChannel::default())
        .insert_resource(SelectedRoom::default())
        // Create room resources
        .insert_resource(CreateRoomConfig::default())
        // Cargar assets embebidos al inicio (antes de todo)
        .add_systems(Startup, load_embedded_assets)
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
            (handle_input, process_network_messages).run_if(in_state(AppState::InGame)),
        )
        // Lógica visual y renderizado (solo en InGame)
        .add_systems(
            Update,
            (
                adjust_field_for_map,
                render_map,
                interpolate_entities,
                keep_name_horizontal,
                keep_l_horizontal,
                keep_r_horizontal,
                camera_follow_player,
                camera_zoom_control,
                update_charge_bar,
                update_player_sprite,
                process_movements,
                update_mode_visuals,
                update_target_ball_position,
                update_dash_cooldown,
                update_camera_viewports,
                spawn_minimap_dots,
                sync_minimap_dots,
                cleanup_minimap_dots,
                draw_minimap_frame,
                update_detail_camera_background,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .run();

    println!("✅ [Bevy] App::run() ha finalizado normalmente");
}

// ============================================================================
// RECURSOS
// ============================================================================

/// Assets embebidos cargados en memoria
#[derive(Resource, Default)]
struct EmbeddedAssets {
    ball_texture: Handle<Image>,
    emoji_font: Handle<Font>,
}

#[derive(Resource, Default)]
struct NetworkChannels {
    receiver: Option<Arc<Mutex<mpsc::Receiver<ServerMessage>>>>,
    sender: Option<mpsc::Sender<PlayerInput>>,
}

#[derive(Resource)]
struct MyPlayerId(Option<u32>);

#[derive(Resource, Default)]
struct LoadedMap(Option<shared::map::Map>);

#[derive(Component)]
struct DefaultFieldLine;

#[derive(Component)]
struct MinimapFieldLine;

#[derive(Component)]
struct FieldBackground;

#[derive(Component)]
struct MenuCamera;

#[derive(Component)]
struct MinimapCamera;

#[derive(Component)]
struct PlayerDetailCamera;

// ============================================================================
// COMPONENTES
// ============================================================================

#[derive(Component)]
struct RemotePlayer {
    id: u32,
    team_index: u8,
    kick_charge: Vec2, // x = potencia, y = curva
    is_sliding: bool,
    not_interacting: bool,
    base_color: Color,
    ball_target_position: Option<Vec2>,
    stamin_charge: f32,
    active_movement: Option<PlayerMovement>,
    mode_active: bool,
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
struct StaminChargeBar;

#[derive(Component)]
struct PlayerNameText;

#[derive(Component)]
struct LeftText;

#[derive(Component)]
struct RightText;

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
struct MinimapFieldBackground;

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
    mut fonts: ResMut<Assets<Font>>,
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

    // Cargar fuente emoji
    let emoji_font =
        Font::try_from_bytes(EMOJI_FONT.to_vec()).expect("Failed to load embedded emoji font");
    let emoji_font_handle = fonts.add(emoji_font);

    commands.insert_resource(EmbeddedAssets {
        ball_texture: ball_handle,
        emoji_font: emoji_font_handle,
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

            ui.heading(egui::RichText::new("RustBall").size(48.0));
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

                ui.add_space(20.0);

                // Botón Configuración
                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Teclas").size(20.0)),
                    )
                    .clicked()
                {
                    next_state.set(AppState::Settings);
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
                    next_state.set(AppState::Menu);
                }
            });
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
    mut config: ResMut<ConnectionConfig>,
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

fn start_connection(config: Res<ConnectionConfig>, mut channels: ResMut<NetworkChannels>) {
    let (network_tx, network_rx) = mpsc::channel();
    let (input_tx, input_rx) = mpsc::channel();

    // Guardar los canales
    channels.receiver = Some(Arc::new(Mutex::new(network_rx)));
    channels.sender = Some(input_tx);

    let ws_url = config.ws_url();
    let room = config.room.clone();
    let player_name = config.player_name.clone();

    // Iniciar hilo de red
    std::thread::spawn(move || {
        println!("🌐 [Red] Iniciando cliente WebRTC");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Fallo al crear Runtime de Tokio");

        rt.block_on(async {
            start_webrtc_client(ws_url, room, player_name, network_tx, input_rx).await;
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
    player_name: String,
    network_tx: mpsc::Sender<ServerMessage>,
    input_rx: mpsc::Receiver<PlayerInput>,
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

    println!("✅ [Red] WebRTC socket creado, esperando conexión con peers...");

    // El server_peer_id real se determina cuando recibimos WELCOME
    let mut server_peer_id: Option<matchbox_socket::PeerId> = None;

    // Rastrear a qué peers ya enviamos JOIN
    let mut peers_joined: std::collections::HashSet<matchbox_socket::PeerId> =
        std::collections::HashSet::new();

    // Loop principal: recibir mensajes y enviar inputs
    loop {
        // Procesar nuevos peers y enviar JOIN a cada uno
        socket.update_peers();
        let current_peers: Vec<_> = socket.connected_peers().collect();

        for peer_id in current_peers {
            if !peers_joined.contains(&peer_id) {
                // Nuevo peer, enviar JOIN
                let join_msg = ControlMessage::Join {
                    player_name: player_name.clone(),
                };
                if let Ok(data) = bincode::serialize(&join_msg) {
                    println!("📤 [Red] Enviando JOIN a peer {:?}...", peer_id);
                    socket.channel_mut(0).send(data.into(), peer_id);
                    peers_joined.insert(peer_id);
                }
            }
        }
        // Recibir mensajes del servidor
        // Canal 0: Control messages (reliable)
        for (peer_id, packet) in socket.channel_mut(0).receive() {
            if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                match msg {
                    ControlMessage::Welcome { player_id, map } => {
                        println!(
                            "🎉 [Red] WELCOME recibido de peer {:?}! Player ID: {}",
                            peer_id, player_id
                        );
                        // Guardar el peer_id del servidor real
                        server_peer_id = Some(peer_id);

                        // Convertir a ServerMessage para compatibilidad con el código existente
                        let server_msg = ServerMessage::Welcome { player_id, map };
                        let _ = network_tx.send(server_msg);

                        // Enviar READY al servidor real
                        let ready_msg = ControlMessage::Ready;
                        if let Ok(data) = bincode::serialize(&ready_msg) {
                            println!("📤 [Red -> Servidor] Enviando READY...");
                            socket.channel_mut(0).send(data.into(), peer_id);
                        }
                    }
                    ControlMessage::PlayerDisconnected { player_id } => {
                        println!("👋 [Red] Jugador {} se desconectó", player_id);
                        let _ = network_tx.send(ServerMessage::PlayerDisconnected { player_id });
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
            while let Ok(input) = input_rx.try_recv() {
                let input_msg = GameDataMessage::Input { input };
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

fn setup(mut commands: Commands, config: Res<GameConfig>) {
    // Cámara principal - Layer 0 (todo excepto minimap dots)
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 3.0, // Reducido de 2.0 para ver el campo más grande
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(0.0, 0.0, 999.0),
        MainCamera,
        RenderLayers::layer(0),
    ));

    // Cámara minimapa - Layer 1 (solo puntos simples)
    // Usa ScalingMode::Fixed para mostrar todo el campo, con deformación si es necesario
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
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
    ));

    // --- Cámara Detalle Jugador (Abajo Derecha) - Layer 0 ---
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.2, 0.2, 0.2)), // Se actualiza dinámicamente
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scale: 1.5, // Zoom alejado para ver jugador más pequeño (3x más lejos)
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(0.0, 0.0, 999.0),
        PlayerDetailCamera,
        RenderLayers::layer(2),
    ));

    // El Campo de Juego (Césped) - Color verde de RustBall - Layer 0
    commands.spawn((
        Sprite {
            color: Color::srgb(0.2, 0.5, 0.2), // RGB(51, 127, 51) - Verde RustBall
            custom_size: Some(Vec2::new(config.arena_width, config.arena_height)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -10.0),
        FieldBackground,
        RenderLayers::layer(0),
    ));

    // Fondo del campo para minimapa - Layer 1 (verde más oscuro)
    commands.spawn((
        Sprite {
            color: Color::srgb(0.1, 0.3, 0.1), // Verde más oscuro para minimapa
            custom_size: Some(Vec2::new(config.arena_width, config.arena_height)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -10.0),
        MinimapFieldBackground,
        RenderLayers::layer(1),
    ));

    // Líneas simplificadas del campo para minimapa - Layer 1
    let minimap_line_thickness = 8.0;
    let minimap_line_color = Color::srgba(1.0, 1.0, 1.0, 0.6);
    let mw = config.arena_width;
    let mh = config.arena_height;

    // Borde superior - minimapa
    commands.spawn((
        Sprite {
            color: minimap_line_color,
            custom_size: Some(Vec2::new(
                mw + minimap_line_thickness,
                minimap_line_thickness,
            )),
            ..default()
        },
        Transform::from_xyz(0.0, mh / 2.0, 0.0),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
    // Borde inferior - minimapa
    commands.spawn((
        Sprite {
            color: minimap_line_color,
            custom_size: Some(Vec2::new(
                mw + minimap_line_thickness,
                minimap_line_thickness,
            )),
            ..default()
        },
        Transform::from_xyz(0.0, -mh / 2.0, 0.0),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
    // Borde izquierdo - minimapa
    commands.spawn((
        Sprite {
            color: minimap_line_color,
            custom_size: Some(Vec2::new(
                minimap_line_thickness,
                mh + minimap_line_thickness,
            )),
            ..default()
        },
        Transform::from_xyz(-mw / 2.0, 0.0, 0.0),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
    // Borde derecho - minimapa
    commands.spawn((
        Sprite {
            color: minimap_line_color,
            custom_size: Some(Vec2::new(
                minimap_line_thickness,
                mh + minimap_line_thickness,
            )),
            ..default()
        },
        Transform::from_xyz(mw / 2.0, 0.0, 0.0),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
    // Línea central vertical - minimapa
    commands.spawn((
        Sprite {
            color: minimap_line_color,
            custom_size: Some(Vec2::new(minimap_line_thickness, mh)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
    // Líneas blancas del campo (bordes) - igual que RustBall (z = 0.0)
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

// Resource para trackear el input anterior
#[derive(Resource, Default)]
struct PreviousInput(PlayerInput);

fn handle_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    channels: Res<NetworkChannels>,
    my_player_id: Res<MyPlayerId>,
    mut previous_input: ResMut<PreviousInput>,
    keybindings: Res<KeyBindingsConfig>,
) {
    if my_player_id.0.is_none() {
        return;
    }

    let Some(ref sender) = channels.sender else {
        return;
    };

    // Detectar modificador (ControlLeft)
    let modifier = keyboard.pressed(KeyCode::ControlLeft);

    // Mapeo de teclas configurable
    let input = PlayerInput {
        move_up: keyboard.pressed(keybindings.move_up.0),
        move_down: keyboard.pressed(keybindings.move_down.0),
        move_left: keyboard.pressed(keybindings.move_left.0),
        move_right: keyboard.pressed(keybindings.move_right.0),
        kick: keyboard.pressed(keybindings.kick.0) && !modifier,
        curve_left: keyboard.pressed(keybindings.curve_left.0),
        curve_right: keyboard.pressed(keybindings.curve_right.0),
        stop_interact: keyboard.pressed(keybindings.stop_interact.0),
        sprint: keyboard.pressed(keybindings.sprint.0) && !modifier,
        dash: modifier && keyboard.pressed(keybindings.sprint.0),
        mode: keyboard.pressed(keybindings.mode.0),
    };

    // Enviamos input siempre, no solo cuando cambia (para mantener estado)
    // El canal unreliable de WebRTC puede perder paquetes, así que enviamos constantemente
    if let Err(e) = sender.send(input.clone()) {
        println!("⚠️ [Bevy] Error enviando input al canal: {:?}", e);
    }
    previous_input.0 = input;
}

fn process_network_messages(
    mut commands: Commands,
    embedded_assets: Res<EmbeddedAssets>,
    mut config: ResMut<GameConfig>,
    channels: Res<NetworkChannels>,
    mut my_id: ResMut<MyPlayerId>,
    mut loaded_map: ResMut<LoadedMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut ball_q: Query<(&mut Interpolated, &mut Transform, &RemoteBall), Without<RemotePlayer>>,
    mut players_q: Query<
        (
            Entity,
            &mut Interpolated,
            &mut Transform,
            &mut RemotePlayer,
            &Children,
        ),
        (Without<RemoteBall>, Without<MainCamera>),
    >,
    // Queries para actualizar colores de equipo
    mut bar_sprites: Query<
        &mut Sprite,
        Or<(With<KickChargeBarCurveLeft>, With<KickChargeBarCurveRight>)>,
    >,
    player_materials: Query<(&PlayerSprite, &MeshMaterial2d<ColorMaterial>)>,
    children_query: Query<&Children>,
    mut text_color_query: Query<&mut TextColor>,
    mut game_tick: ResMut<GameTick>,
) {
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
            // Igual que RustBall: usar textura con children
            commands
                .spawn((
                    Transform::from_xyz(ball.position.0, ball.position.1, 0.0),
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
                    rp.mode_active = ps.mode_active;

                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);

                // Determinar si es el jugador local
                let is_local = my_id.0 == Some(ps.id);
                let player_layers = if is_local {
                    RenderLayers::from_layers(&[0, 2]) // Visible en cámara principal y detalle
                } else {
                    RenderLayers::layer(0) // Solo visible en cámara principal
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

                // Igual que RustBall: usar textura con children
                commands
                    .spawn((
                        Transform::from_xyz(ps.position.x, ps.position.y, 0.0),
                        Visibility::default(),
                        RemotePlayer {
                            id: ps.id,
                            team_index: ps.team_index,
                            kick_charge: ps.kick_charge,
                            is_sliding: ps.is_sliding,
                            not_interacting: ps.not_interacting,
                            base_color: player_color,
                            ball_target_position: ps.ball_target_position,
                            stamin_charge: ps.stamin_charge,
                            active_movement: ps.active_movement.clone(),
                            mode_active: ps.mode_active,
                        },
                        Collider::ball(config.sphere_radius), // Para debug rendering
                        Interpolated {
                            target_position: ps.position,
                            target_velocity: Vec2::new(ps.velocity.0, ps.velocity.1),
                            target_rotation: ps.rotation,
                            smoothing: 15.0,
                        },
                        player_layers.clone(),
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
                            player_layers.clone(),
                        ));

                        // Círculo principal (relleno) - color del jugador
                        parent.spawn((
                            Mesh2d(meshes.add(Circle::new(radius))),
                            MeshMaterial2d(materials.add(player_color)),
                            Transform::from_xyz(0.0, 0.0, 1.0),
                            PlayerSprite { parent_id: ps.id },
                            player_layers.clone(),
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
                            player_layers.clone(),
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
                            player_layers.clone(),
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
                            player_layers.clone(),
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
                            player_layers.clone(),
                        ));

                        let angle_90 = 90.0f32.to_radians();

                        // Solo para jugador local: barra de estamina
                        if is_local {
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
                                RenderLayers::layer(2),
                            ));
                        }

                        // Nombre del jugador debajo del sprite
                        parent.spawn((
                            PlayerNameText,
                            Text2d::new(ps.name.clone()),
                            TextFont {
                                font_size: 20.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                            Transform::from_xyz(-config.sphere_radius * 1.5, 0.0, 10.0),
                            RenderLayers::layer(0),
                        ));

                        // Solo para jugador local: indicadores de curva
                        if is_local {
                            parent.spawn((
                                LeftText,
                                Text2d::new("I"),
                                TextFont {
                                    font: embedded_assets.emoji_font.clone(),
                                    font_size: 30.0,
                                    ..default()
                                },
                                Transform {
                                    translation: Vec3::new(0.0, config.ball_radius * 4.0, 10.0),
                                    rotation: Quat::from_rotation_z(angle_90),
                                    ..default()
                                },
                                RenderLayers::layer(2),
                            ));
                            parent.spawn((
                                RightText,
                                Text2d::new("D"),
                                TextFont {
                                    font: embedded_assets.emoji_font.clone(),
                                    font_size: 30.0,
                                    ..default()
                                },
                                Transform {
                                    translation: Vec3::new(0.0, -config.ball_radius * 4.0, 10.0),
                                    rotation: Quat::from_rotation_z(angle_90),
                                    ..default()
                                },
                                RenderLayers::layer(2),
                            ));
                        }
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

fn keep_l_horizontal(
    mut l_query: Query<(&mut Transform, &ChildOf), With<LeftText>>,
    parent_query: Query<&Transform, (With<RemotePlayer>, Without<LeftText>)>,
) {
    for (mut l_transform, child_of) in l_query.iter_mut() {
        if let Ok(parent_transform) = parent_query.get(child_of.parent()) {
            // Contrarrestar la rotación del padre para que el texto quede horizontal
            l_transform.rotation = parent_transform.rotation.inverse();
        }
    }
}

fn keep_r_horizontal(
    mut r_query: Query<(&mut Transform, &ChildOf), With<RightText>>,
    parent_query: Query<&Transform, (With<RemotePlayer>, Without<RightText>)>,
) {
    for (mut r_transform, child_of) in r_query.iter_mut() {
        if let Ok(parent_transform) = parent_query.get(child_of.parent()) {
            // Contrarrestar la rotación del padre para que el texto quede horizontal
            r_transform.rotation = parent_transform.rotation.inverse();
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

fn camera_follow_player(
    my_player_id: Res<MyPlayerId>,
    players: Query<
        (&RemotePlayer, &Transform),
        (
            Without<MainCamera>,
            Without<MinimapCamera>,
            Without<PlayerDetailCamera>,
        ),
    >,
    mut cameras: ParamSet<(
        Query<&mut Transform, With<MainCamera>>,
        Query<&mut Transform, With<MinimapCamera>>,
        Query<&mut Transform, With<PlayerDetailCamera>>,
    )>,
) {
    let Some(my_id) = my_player_id.0 else { return };

    // Buscamos la posición de nuestro jugador
    let player_pos = players
        .iter()
        .find(|(p, _)| p.id == my_id)
        .map(|(_, t)| t.translation);

    if let Some(pos) = player_pos {
        // 1. Cámara principal sigue al jugador
        if let Ok(mut cam) = cameras.p0().single_mut() {
            cam.translation.x = pos.x;
            cam.translation.y = pos.y;
        }

        // 3. Detalle del jugador sigue al jugador (siempre centrado en él)
        if let Ok(mut cam) = cameras.p2().single_mut() {
            cam.translation.x = pos.x;
            cam.translation.y = pos.y;
        }
    }
}

// Sistema de control de zoom con teclas numéricas
fn camera_zoom_control(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut camera: Query<&mut Projection, With<MainCamera>>,
) {
    let Ok(mut projection_comp) = camera.single_mut() else {
        return;
    };
    let Projection::Orthographic(ref mut projection) = projection_comp.as_mut() else {
        return;
    };
    {
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
            projection.scale = scale;
            println!("📷 Zoom ajustado a: {:.1}x", scale);
        }
    }
}

use bevy::camera::Viewport;

fn update_camera_viewports(
    windows: Query<&Window>,
    mut minimap_q: Query<&mut Camera, (With<MinimapCamera>, Without<PlayerDetailCamera>)>,
    mut detail_q: Query<&mut Camera, (With<PlayerDetailCamera>, Without<MinimapCamera>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let width = window.resolution.physical_width();
    let height = window.resolution.physical_height();

    // Minimapa: 300x180 px, centrado horizontalmente abajo
    if let Ok(mut cam) = minimap_q.single_mut() {
        let size = UVec2::new(300, 180);
        cam.viewport = Some(Viewport {
            physical_position: UVec2::new((width / 2) - (size.x / 2), height - size.y - 15),
            physical_size: size,
            depth: 0.0..1.0,
        });
    }

    // Detalle Jugador: 200x200 px, abajo a la derecha
    if let Ok(mut cam) = detail_q.single_mut() {
        let size = UVec2::new(200, 200);
        cam.viewport = Some(Viewport {
            physical_position: UVec2::new(width - size.x - 15, height - size.y - 15),
            physical_size: size,
            depth: 0.0..1.0,
        });
    }
}

fn update_charge_bar(
    player_query: Query<(&RemotePlayer, &Children)>,
    // Una sola query mutable para el Sprite evita el conflicto B0001
    mut sprite_query: Query<&mut Sprite>,
    // Queries de solo lectura para identificar qué tipo de barra es cada hijo
    bar_main_q: Query<Entity, With<KickChargeBar>>,
    bar_left_q: Query<Entity, With<KickChargeBarCurveLeft>>,
    bar_right_q: Query<Entity, With<KickChargeBarCurveRight>>,
) {
    let max_width = 45.0;

    for (player, children) in player_query.iter() {
        for child in children.iter() {
            // Intentamos obtener el sprite del hijo
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                // 1. Caso: Barra Principal
                if bar_main_q.contains(child) {
                    sprite.custom_size =
                        Some(Vec2::new(max_width * player.kick_charge.x + 5.0, 5.0));
                }
                // 2. Caso: Curva Izquierda (kick_charge.y < 0)
                else if bar_left_q.contains(child) {
                    let coeficient = if player.kick_charge.y < 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(
                        max_width * player.kick_charge.x * coeficient + 5.0,
                        5.0,
                    ));
                }
                // 3. Caso: Curva Derecha (kick_charge.y > 0)
                else if bar_right_q.contains(child) {
                    let coeficient = if player.kick_charge.y > 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(
                        max_width * player.kick_charge.x * coeficient + 5.0,
                        5.0,
                    ));
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

                if player.mode_active {
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

                if player.mode_active {
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
                if let Ok((mut sprite, _transform)) = field_bg.single_mut() {
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
        let pos = Vec2::new(vertex.x, vertex.y);
        gizmos.circle_2d(pos, 3.0, vertex_color); // Radio pequeño 3.0
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

        let p0 = Vec2::new(v0.x, v0.y);
        let p1 = Vec2::new(v1.x, v1.y);

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
            gizmos.line_2d(p0, p1, line_color);
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
                gizmos.line_2d(points[i], points[i + 1], line_color);
            }
        }
    }

    // Dibujar discos (obstáculos circulares)
    for disc in &map.discs {
        let pos = Vec2::new(disc.pos[0], disc.pos[1]);
        gizmos.circle_2d(pos, disc.radius, disc_color);
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

// ============================================================================
// SISTEMAS DE MINIMAPA
// ============================================================================

/// Crea puntos en Layer 1 cuando aparecen jugadores/pelota
fn spawn_minimap_dots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    config: Res<GameConfig>,
    existing_dots: Query<&MinimapDot>,
    players_with_dots: Query<(Entity, &RemotePlayer)>,
    ball_with_dots: Query<Entity, With<RemoteBall>>,
) {
    // Crear set de entidades ya trackeadas
    let tracked_entities: std::collections::HashSet<Entity> =
        existing_dots.iter().map(|dot| dot.tracks_entity).collect();

    // Spawn dots para jugadores que aún no tienen
    for (entity, player) in players_with_dots.iter() {
        if tracked_entities.contains(&entity) {
            continue;
        }

        // Color del equipo desde config
        let team_color = config
            .team_colors
            .get(player.team_index as usize)
            .copied()
            .unwrap_or((0.5, 0.5, 0.5));

        let dot_color = Color::srgb(team_color.0, team_color.1, team_color.2);

        // Círculo de 80px para jugadores (4x más grande para minimapa)
        commands.spawn((
            Mesh2d(meshes.add(Circle::new(80.0))),
            MeshMaterial2d(materials.add(dot_color)),
            Transform::from_xyz(0.0, 0.0, 10.0),
            MinimapDot {
                tracks_entity: entity,
            },
            RenderLayers::layer(1),
        ));
    }

    // Spawn dot para pelota si no tiene
    for entity in ball_with_dots.iter() {
        if tracked_entities.contains(&entity) {
            continue;
        }

        // Círculo de 60px blanco para pelota (4x más grande para minimapa)
        commands.spawn((
            Mesh2d(meshes.add(Circle::new(60.0))),
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
    entities: Query<Entity>,
) {
    for (dot_entity, dot) in dots.iter() {
        // Si la entidad trackeada ya no existe, eliminar el dot
        if entities.get(dot.tracks_entity).is_err() {
            commands.entity(dot_entity).despawn();
        }
    }
}

/// Dibuja un marco alrededor del minimapa usando egui
fn draw_minimap_frame(mut contexts: EguiContexts, windows: Query<&Window>) {
    let Ok(window) = windows.single() else {
        return;
    };

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Dimensiones del minimapa (mismas que en update_camera_viewports)
    let minimap_width = 300.0;
    let minimap_height = 180.0;
    let margin = 15.0;
    let frame_thickness = 3.0;

    // Posición del minimapa (centrado horizontalmente, abajo)
    let window_width = window.width();
    let window_height = window.height();
    let minimap_x = (window_width - minimap_width) / 2.0;
    let minimap_y = window_height - minimap_height - margin;

    // Dibujar marco usando egui::Area
    egui::Area::new(egui::Id::new("minimap_frame"))
        .fixed_pos(egui::pos2(
            minimap_x - frame_thickness,
            minimap_y - frame_thickness,
        ))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            let frame_size = egui::vec2(
                minimap_width + frame_thickness * 2.0,
                minimap_height + frame_thickness * 2.0,
            );

            // Marco con borde blanco y fondo semitransparente
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180))
                .stroke(egui::Stroke::new(2.0, egui::Color32::WHITE))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.allocate_space(frame_size);
                });
        });
}

/// Actualiza el fondo de la cámara de detalle al color complementario del equipo del jugador
fn update_detail_camera_background(
    my_player_id: Res<MyPlayerId>,
    config: Res<GameConfig>,
    players: Query<&RemotePlayer>,
    mut camera_q: Query<&mut Camera, With<PlayerDetailCamera>>,
) {
    let Some(my_id) = my_player_id.0 else { return };

    // Buscar mi jugador para obtener el team_index
    let Some(my_player) = players.iter().find(|p| p.id == my_id) else {
        return;
    };

    // Obtener color del equipo
    let team_color = config
        .team_colors
        .get(my_player.team_index as usize)
        .copied()
        .unwrap_or((0.5, 0.5, 0.5));

    // Calcular color complementario
    let (r, g, b) = complementary_color(team_color.0, team_color.1, team_color.2);

    // Actualizar clear_color de la cámara
    if let Ok(mut cam) = camera_q.single_mut() {
        cam.clear_color = ClearColorConfig::Custom(Color::srgb(r, g, b));
    }
}
