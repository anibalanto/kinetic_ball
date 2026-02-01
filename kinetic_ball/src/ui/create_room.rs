use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::resources::CreateRoomConfig;
use crate::states::AppState;

pub fn create_room_ui(
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
                next_state.set(AppState::RoomSelection);
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
