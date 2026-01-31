use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::keybindings::{
    key_code_display_name, save_keybindings, GameAction, KeyBindingsConfig, SettingsUIState,
};
use crate::states::AppState;

/// Sistema de UI para configuración de teclas
pub fn settings_ui(
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
