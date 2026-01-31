use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::keybindings::{
    save_gamepad_bindings_map, DetectedGamepadEvent, GameAction, GamepadBindingsConfig,
    GamepadBindingsMap, GamepadConfigUIState,
};
use crate::states::AppState;

/// Sistema de UI para configuración de gamepad
pub fn gamepad_config_ui(
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
