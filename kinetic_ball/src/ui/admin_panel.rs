use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::components::RemotePlayer;
use crate::local_players::LocalPlayers;
use crate::resources::{AdminPanelState, ConnectionConfig, NetworkChannels};
use crate::shared::protocol::ControlMessage;
use crate::states::AppState;

/// Sistema que detecta Escape para toggle del panel de admin
pub fn toggle_admin_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut admin_state: ResMut<AdminPanelState>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        admin_state.is_open = !admin_state.is_open;
        println!(
            "🔧 Admin panel: {}",
            if admin_state.is_open {
                "ABIERTO"
            } else {
                "CERRADO"
            }
        );
    }
}

/// UI del panel de administración
pub fn admin_panel_ui(
    mut contexts: EguiContexts,
    mut admin_state: ResMut<AdminPanelState>,
    config: Res<ConnectionConfig>,
    players_q: Query<&RemotePlayer>,
    mut next_state: ResMut<NextState<AppState>>,
    local_players: Res<LocalPlayers>,
    channels: Res<NetworkChannels>,
) {
    if !admin_state.is_open {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::Window::new("Administración")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(400.0);

            // UUID de la sala
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Room ID:");
                    ui.label(egui::RichText::new(&config.room).monospace());
                    if ui.button("📋").on_hover_text("Copiar").clicked() {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(&config.room);
                        }
                    }
                });
            });

            ui.add_space(10.0);

            // Lista de jugadores
            ui.group(|ui| {
                ui.heading("Jugadores");
                ui.add_space(5.0);

                let mut players: Vec<_> = players_q.iter().collect();
                players.sort_by_key(|p| p.id);

                for player in players {
                    let is_selected = admin_state.selected_player_id == Some(player.id);

                    let frame = if is_selected {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(60, 80, 120))
                            .inner_margin(5.0)
                            .corner_radius(3.0)
                    } else {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(40, 40, 50))
                            .inner_margin(5.0)
                            .corner_radius(3.0)
                    };

                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Indicador de equipo
                            let team_color = match player.team_index {
                                0 => egui::Color32::from_rgb(230, 50, 50),
                                1 => egui::Color32::from_rgb(50, 100, 230),
                                _ => egui::Color32::GRAY,
                            };
                            ui.colored_label(team_color, "●");

                            // Nombre del jugador
                            if ui.selectable_label(is_selected, &player.name).clicked() {
                                admin_state.selected_player_id = Some(player.id);
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("ID:{}", player.id))
                                            .size(10.0)
                                            .color(egui::Color32::GRAY),
                                    );
                                },
                            );
                        });
                    });

                    ui.add_space(2.0);
                }

                if players_q.is_empty() {
                    ui.label(egui::RichText::new("No hay jugadores").color(egui::Color32::GRAY));
                }
            });

            ui.add_space(10.0);

            // Acciones sobre jugador seleccionado
            if let Some(player_id) = admin_state.selected_player_id {
                ui.group(|ui| {
                    ui.heading("Acciones");
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        if ui.button("Equipo Rojo").clicked() {
                            println!("🔴 Cambiar jugador {} a equipo Rojo", player_id);
                            // TODO: Enviar mensaje al servidor
                        }
                        if ui.button("Equipo Azul").clicked() {
                            println!("🔵 Cambiar jugador {} a equipo Azul", player_id);
                            // TODO: Enviar mensaje al servidor
                        }
                    });

                    ui.add_space(5.0);

                    if ui
                        .button(egui::RichText::new("Expulsar Jugador").color(egui::Color32::RED))
                        .clicked()
                    {
                        println!("👢 Expulsar jugador {}", player_id);
                        // TODO: Enviar mensaje al servidor
                    }
                });

                ui.add_space(10.0);
            }

            // Acciones de sala
            ui.separator();
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Salir de la Sala").clicked() {
                    println!("🚪 Saliendo de la sala");

                    // Enviar mensaje Leave para cada jugador local
                    if let Some(ref control_tx) = channels.control_sender {
                        for lp in &local_players.players {
                            if let Some(player_id) = lp.server_player_id {
                                let _ = control_tx.send(ControlMessage::Leave { player_id });
                            }
                        }
                    }

                    admin_state.is_open = false;
                    next_state.set(AppState::RoomSelection);
                }

                if ui
                    .button(egui::RichText::new("Cerrar Sala").color(egui::Color32::RED))
                    .clicked()
                {
                    println!("🔒 Cerrando la sala");
                    // TODO: Enviar mensaje al servidor para cerrar
                    admin_state.is_open = false;
                    next_state.set(AppState::RoomSelection);
                }
            });

            ui.add_space(10.0);

            // Botón cerrar
            ui.separator();
            if ui.button("Cerrar (Esc)").clicked() {
                admin_state.is_open = false;
            }
        });
}
