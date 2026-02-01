use bevy::prelude::*;
use bevy::sprite_render::Material2dPlugin;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use bevy_rapier2d::prelude::*;
use clap::Parser;

// ============================================================================
// MODULES
// ============================================================================

mod assets;
mod camera;
mod color_utils;
mod components;
mod events;
mod game;
mod host;
mod keybindings;
mod local_players;
mod networking;
mod rendering;
mod resources;
mod shared;
mod spawning;
mod states;
mod ui;

// ============================================================================
// RE-EXPORTS
// ============================================================================

use assets::{load_embedded_assets, EmbeddedAssets};
use camera::{
    camera_follow_player_and_ball, camera_zoom_control, update_camera_viewports,
    update_split_compositor, update_split_screen_state,
};
use events::{SpawnBallEvent, SpawnPlayerEvent};
use game::{
    animate_keys, cleanup_game, handle_multi_player_input, interpolate_entities, process_movements,
    setup,
};
use keybindings::{
    load_app_config, load_gamepad_bindings_map, load_keybindings, DetectedGamepadEvent,
    GamepadBindingsMap, GamepadConfigUIState, GilrsWrapper, KeyBindingsConfig, RawGamepadInput,
    SettingsUIState,
};
use local_players::{detect_gamepads, AvailableInputDevices, LocalPlayers, LocalPlayersUIState};
use networking::{check_connection, process_network_messages, start_connection};
use rendering::{
    adjust_field_for_map, cleanup_minimap_dots, keep_name_horizontal, spawn_minimap_dots,
    sync_minimap_dots, sync_minimap_names, update_charge_bar, update_dash_cooldown,
    update_mode_visuals, update_player_sprite,
};
use resources::{
    AdminPanelState, ConnectionConfig, CreateRoomConfig, DynamicSplitState, GameTick, LoadedMap,
    MyPlayerId, NetworkChannels, PlayerColors, PreviousInput, RoomFetchChannel, RoomList,
    SelectedRoom, SplitScreenMaterial, SplitScreenTextures,
};
use shared::protocol::GameConfig;
use spawning::{handle_spawn_ball, handle_spawn_player};
use states::AppState;
use ui::{
    admin_panel_ui, check_rooms_fetch, cleanup_menu_camera, create_room_ui, fetch_rooms,
    gamepad_config_ui, hosting_ui, local_players_setup_ui, menu_ui, room_selection_ui, settings_ui,
    setup_menu_camera_if_needed, start_hosting, toggle_admin_panel,
};

// ============================================================================
// CLI ARGUMENTS
// ============================================================================

#[derive(Parser, Debug, Clone)]
#[command(name = "Haxball Client")]
#[command(about = "Cliente del juego Haxball", long_about = None)]
pub struct Args {
    /// Host del proxy (sin protocolo). Ejemplo: localhost:3537 o proxy.ejemplo.com
    #[arg(short, long)]
    pub server: Option<String>,

    /// Nombre de la sala/room
    #[arg(short, long, default_value = "game_server")]
    pub room: String,

    /// Nombre del jugador
    #[arg(long, default_value = "Player")]
    pub name: String,
}

// ============================================================================
// MAIN
// ============================================================================

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
        // Admin panel state
        .insert_resource(AdminPanelState::default())
        // Eventos de spawning
        .add_event::<SpawnBallEvent>()
        .add_event::<SpawnPlayerEvent>()
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
        // Cleanup al salir de InGame
        .add_systems(OnExit(AppState::InGame), cleanup_game)
        // Lógica de red y entrada (frecuencia fija, solo en InGame)
        .add_systems(
            FixedUpdate,
            (handle_multi_player_input, process_network_messages)
                .run_if(in_state(AppState::InGame)),
        )
        // Sistemas de spawning (procesan eventos emitidos por network)
        .add_systems(
            Update,
            (handle_spawn_ball, handle_spawn_player).run_if(in_state(AppState::InGame)),
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
        // Panel de administración - sin run_if para debug
        .add_systems(
            Update,
            toggle_admin_panel.run_if(in_state(AppState::InGame)),
        )
        .add_systems(EguiPrimaryContextPass, admin_panel_ui)
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
