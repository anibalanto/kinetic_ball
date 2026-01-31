use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::assets::DEFAULT_MAP;
use crate::host;
use crate::resources::{ConnectionConfig, CreateRoomConfig};
use crate::states::AppState;

pub fn start_hosting(config: Res<ConnectionConfig>, create_config: Res<CreateRoomConfig>) {
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

pub fn hosting_ui(
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
