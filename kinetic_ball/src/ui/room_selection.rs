use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::resources::{ConnectionConfig, RoomFetchChannel, RoomList, SelectedRoom};
use crate::states::{AppState, RoomInfo, RoomStatus};

pub fn fetch_rooms(
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

pub fn check_rooms_fetch(mut room_list: ResMut<RoomList>, mut fetch_channel: ResMut<RoomFetchChannel>) {
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

pub fn room_selection_ui(
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
                                if let Some(ref version) = room.min_version {
                                    ui.label(
                                        egui::RichText::new(format!("v{}", version))
                                            .size(12.0)
                                            .color(egui::Color32::LIGHT_BLUE),
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
