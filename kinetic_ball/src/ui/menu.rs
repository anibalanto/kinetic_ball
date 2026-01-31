use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::components::MenuCamera;
use crate::resources::ConnectionConfig;
use crate::states::AppState;

pub fn setup_menu_camera_if_needed(mut commands: Commands, menu_camera: Query<&MenuCamera>) {
    // Solo crear cámara si no existe
    if menu_camera.is_empty() {
        commands.spawn((Camera2d, MenuCamera));
    }
}

pub fn cleanup_menu_camera(mut commands: Commands, menu_camera: Query<Entity, With<MenuCamera>>) {
    for entity in menu_camera.iter() {
        commands.entity(entity).despawn();
    }
}

pub fn menu_ui(
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
