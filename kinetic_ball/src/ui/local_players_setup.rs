use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::keybindings::{GamepadBindingsMap, GamepadConfigUIState, GilrsWrapper};
use crate::local_players::{AvailableInputDevices, InputDevice, LocalPlayers, LocalPlayersUIState};
use crate::resources::ConnectionConfig;
use crate::states::AppState;

/// Sistema de UI para configurar jugadores locales
pub fn local_players_setup_ui(
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
