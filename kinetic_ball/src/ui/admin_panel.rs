use std::collections::HashSet;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::components::RemotePlayer;
use crate::local_players::LocalPlayers;
use crate::resources::{AdminPanelState, ClientMatchSlots, ConnectionConfig, NetworkChannels};
use crate::shared::protocol::ControlMessage;
use crate::states::AppState;

// Background colors for drop zones
const RED_STARTER_BG: egui::Color32 = egui::Color32::from_rgb(120, 40, 40);
const RED_SUB_BG: egui::Color32 = egui::Color32::from_rgb(80, 30, 30);
const BLUE_STARTER_BG: egui::Color32 = egui::Color32::from_rgb(40, 60, 120);
const BLUE_SUB_BG: egui::Color32 = egui::Color32::from_rgb(30, 40, 80);
const SPECTATOR_BG: egui::Color32 = egui::Color32::from_rgb(60, 60, 60);

/// Sistema que detecta Escape para toggle del panel de admin
pub fn toggle_admin_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut admin_state: ResMut<AdminPanelState>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        admin_state.is_open = !admin_state.is_open;
    }
}

/// Helper struct to collect player info for display
#[derive(Clone)]
struct PlayerInfo {
    id: u32,
    name: String,
}

/// Gets player name from the list, or returns a placeholder
fn get_player_name(player_id: u32, all_players: &[PlayerInfo]) -> String {
    all_players
        .iter()
        .find(|p| p.id == player_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| format!("Player#{}", player_id))
}

/// UI del panel de administración con 3 columnas y drag & drop
pub fn admin_panel_ui(
    mut contexts: EguiContexts,
    mut admin_state: ResMut<AdminPanelState>,
    config: Res<ConnectionConfig>,
    players_q: Query<&RemotePlayer>,
    mut next_state: ResMut<NextState<AppState>>,
    local_players: Res<LocalPlayers>,
    channels: Res<NetworkChannels>,
    match_slots: Res<ClientMatchSlots>,
) {
    if !admin_state.is_open {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };

    // Collect all player info
    let all_players: Vec<PlayerInfo> = players_q
        .iter()
        .map(|p| PlayerInfo {
            id: p.id,
            name: p.name.clone(),
        })
        .collect();

    let slots = &match_slots.0;
    let is_admin = admin_state.is_admin;

    egui::Window::new("Admin")
        .collapsible(true)
        .resizable(false)
        .default_width(600.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Room ID header
            ui.horizontal(|ui| {
                ui.label("Room:");
                ui.label(egui::RichText::new(&config.room).monospace().small());
                if ui.small_button("📋").on_hover_text("Copiar").clicked() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&config.room);
                    }
                }
                if is_admin {
                    ui.label(egui::RichText::new("👑").color(egui::Color32::GOLD));
                }
            });

            ui.separator();

            // Three column layout with scroll
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Column 1: Red Team
                        ui.vertical(|ui| {
                            ui.set_min_width(180.0);
                            ui.set_max_width(180.0);
                            ui.label(
                                egui::RichText::new("🔴 ROJO")
                                    .color(egui::Color32::from_rgb(230, 80, 80))
                                    .strong(),
                            );

                            // Drop zone: Red Starters
                            if let Some(player_id) = render_drop_zone(
                                ui,
                                "red-starters",
                                "Titulares",
                                &slots.teams[0].starters,
                                &all_players,
                                &slots.admins,
                                is_admin,
                                RED_STARTER_BG,
                                &channels,
                            ) {
                                send_move_player(&channels, player_id, Some(0), Some(true));
                            }

                            ui.add_space(5.0);

                            // Drop zone: Red Substitutes
                            if let Some(player_id) = render_drop_zone(
                                ui,
                                "red-subs",
                                "Suplentes",
                                &slots.teams[0].substitutes,
                                &all_players,
                                &slots.admins,
                                is_admin,
                                RED_SUB_BG,
                                &channels,
                            ) {
                                send_move_player(&channels, player_id, Some(0), Some(false));
                            }
                        });

                        ui.separator();

                        // Column 2: Spectators
                        ui.vertical(|ui| {
                            ui.set_min_width(180.0);
                            ui.set_max_width(180.0);
                            ui.label(egui::RichText::new("👁 ESPECTADORES").strong());

                            // Drop zone: Spectators
                            if let Some(player_id) = render_drop_zone(
                                ui,
                                "spectators",
                                "",
                                &slots.spectators,
                                &all_players,
                                &slots.admins,
                                is_admin,
                                SPECTATOR_BG,
                                &channels,
                            ) {
                                send_move_player(&channels, player_id, None, None);
                            }
                        });

                        ui.separator();

                        // Column 3: Blue Team
                        ui.vertical(|ui| {
                            ui.set_min_width(180.0);
                            ui.set_max_width(180.0);
                            ui.label(
                                egui::RichText::new("🔵 AZUL")
                                    .color(egui::Color32::from_rgb(80, 120, 230))
                                    .strong(),
                            );

                            // Drop zone: Blue Starters
                            if let Some(player_id) = render_drop_zone(
                                ui,
                                "blue-starters",
                                "Titulares",
                                &slots.teams[1].starters,
                                &all_players,
                                &slots.admins,
                                is_admin,
                                BLUE_STARTER_BG,
                                &channels,
                            ) {
                                send_move_player(&channels, player_id, Some(1), Some(true));
                            }

                            ui.add_space(5.0);

                            // Drop zone: Blue Substitutes
                            if let Some(player_id) = render_drop_zone(
                                ui,
                                "blue-subs",
                                "Suplentes",
                                &slots.teams[1].substitutes,
                                &all_players,
                                &slots.admins,
                                is_admin,
                                BLUE_SUB_BG,
                                &channels,
                            ) {
                                send_move_player(&channels, player_id, Some(1), Some(false));
                            }
                        });
                    });
                });

            ui.separator();

            // Room actions
            ui.horizontal(|ui| {
                if ui.button("Salir").clicked() {
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

                if ui.button("Cerrar (Esc)").clicked() {
                    admin_state.is_open = false;
                }
            });
        });
}

/// Renders a drop zone that can receive dragged players
fn render_drop_zone(
    ui: &mut egui::Ui,
    zone_id: &str,
    label: &str,
    player_ids: &HashSet<u32>,
    all_players: &[PlayerInfo],
    admins: &HashSet<u32>,
    is_admin: bool,
    bg_color: egui::Color32,
    channels: &NetworkChannels,
) -> Option<u32> {
    let frame = egui::Frame::new()
        .fill(bg_color)
        .inner_margin(4.0)
        .corner_radius(4.0);

    let (_, dropped_payload) = ui.dnd_drop_zone::<u32, ()>(frame, |ui| {
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).small());
        }

        if player_ids.is_empty() {
            ui.label(
                egui::RichText::new("(vacío)")
                    .color(egui::Color32::DARK_GRAY)
                    .small()
                    .italics(),
            );
        } else {
            // Sort player IDs for consistent display
            let mut sorted_ids: Vec<_> = player_ids.iter().copied().collect();
            sorted_ids.sort();

            for player_id in sorted_ids {
                render_draggable_player(ui, player_id, all_players, admins, is_admin, channels);
            }
        }
    });

    // Return the player_id if something was dropped
    dropped_payload.map(|p| *p)
}

/// Renders a single draggable player item
fn render_draggable_player(
    ui: &mut egui::Ui,
    player_id: u32,
    all_players: &[PlayerInfo],
    admins: &HashSet<u32>,
    is_admin: bool,
    channels: &NetworkChannels,
) {
    let player_name = get_player_name(player_id, all_players);
    let is_player_admin = admins.contains(&player_id);

    if is_admin {
        // Admin can drag players
        let id = egui::Id::new(format!("player-{}", player_id));
        let response = ui
            .dnd_drag_source(id, player_id, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("≡").small()); // Drag indicator
                    if is_player_admin {
                        ui.label(egui::RichText::new("👑").small());
                    }
                    ui.label(egui::RichText::new(&player_name).small());
                });
            })
            .response;

        // Context menu for admin actions
        response.context_menu(|ui| {
            if is_player_admin {
                if ui.button("👑 Quitar Admin").clicked() {
                    send_toggle_admin(channels, player_id, false);
                    ui.close_menu();
                }
            } else {
                if ui.button("👑 Dar Admin").clicked() {
                    send_toggle_admin(channels, player_id, true);
                    ui.close_menu();
                }
            }

            if ui
                .button(egui::RichText::new("👢 Expulsar").color(egui::Color32::RED))
                .clicked()
            {
                send_kick_player(channels, player_id);
                ui.close_menu();
            }
        });
    } else {
        // Non-admin can only view
        ui.horizontal(|ui| {
            if is_player_admin {
                ui.label(egui::RichText::new("👑").small());
            }
            ui.label(egui::RichText::new(&player_name).small());
        });
    }
}

/// Sends a MovePlayer control message
fn send_move_player(
    channels: &NetworkChannels,
    player_id: u32,
    team_index: Option<u8>,
    is_starter: Option<bool>,
) {
    if let Some(ref control_tx) = channels.control_sender {
        let msg = ControlMessage::MovePlayer {
            player_id,
            team_index,
            is_starter,
        };
        let _ = control_tx.send(msg);
    }
}

/// Sends a KickPlayer control message
fn send_kick_player(channels: &NetworkChannels, player_id: u32) {
    if let Some(ref control_tx) = channels.control_sender {
        let msg = ControlMessage::KickPlayer { player_id };
        let _ = control_tx.send(msg);
    }
}

/// Sends a ToggleAdmin control message
fn send_toggle_admin(channels: &NetworkChannels, player_id: u32, is_admin: bool) {
    if let Some(ref control_tx) = channels.control_sender {
        let msg = ControlMessage::ToggleAdmin { player_id, is_admin };
        let _ = control_tx.send(msg);
    }
}
