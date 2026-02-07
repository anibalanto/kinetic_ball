use crate::networking::hmac_auth;
use crate::shared::*;
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use matchbox_socket::{PeerId, PeerState, WebRtcSocket};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::engine::spawn_physics;
use super::host::{
    Ball, BroadcastTimer, GameInputManager, GameTick, HostMatchSlots, LoadedMap, NetworkEvent,
    NetworkReceiver, NetworkSender, NetworkState, OutgoingMessage, Player, Sphere,
};

// ============================================================================
// NETWORK SERVER - MATCHBOX WEBRTC
// ============================================================================

pub fn start_webrtc_server(
    event_tx: mpsc::Sender<NetworkEvent>,
    state: Arc<Mutex<NetworkState>>,
    room: String,
    outgoing_rx: mpsc::Receiver<OutgoingMessage>,
    server_host: String,
    room_name: String,
    max_players: u8,
    map_name: Option<String>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("No se pudo crear el runtime de Tokio");

    rt.block_on(async {
        // Registrar la room en el proxy y obtener token
        // Usar HTTPS/WSS para servidores remotos, HTTP/WS para localhost
        let is_localhost =
            server_host.starts_with("127.0.0.1") || server_host.starts_with("localhost");
        let http_scheme = if is_localhost { "http" } else { "https" };
        let ws_scheme = if is_localhost { "ws" } else { "wss" };
        let http_url = format!("{}://{}", http_scheme, server_host);
        // Obtener versión mínima del servidor para enviar al proxy
        let min_version_str = protocol::ProtocolVersion::current().to_string();
        let room_url = match register_room_with_proxy(
            &http_url,
            &room,
            &room_name,
            max_players,
            map_name.as_deref(),
            Some(&min_version_str),
        )
        .await
        {
            Ok(token) => {
                println!("✅ Room '{}' registrada en proxy", room);
                let ws_url = format!("{}://{}", ws_scheme, server_host);
                format!("{}/connect?token={}", ws_url, token)
            }
            Err(e) => {
                eprintln!("❌ Error registrando room en proxy: {}", e);
                eprintln!("   Asegúrate de que el proxy está corriendo");
                return;
            }
        };

        println!("🔗 Connecting to: {}", room_url);

        // Crear WebRtcSocket y conectar a la room
        let (mut socket, loop_fut) = WebRtcSocket::builder(room_url)
            .add_channel(matchbox_socket::ChannelConfig::reliable()) // Canal 0: Control (reliable)
            .add_channel(matchbox_socket::ChannelConfig::unreliable()) // Canal 1: GameData (unreliable)
            .build();

        // Spawn el loop de matchbox (maneja la señalización)
        tokio::spawn(loop_fut);

        println!("✅ Server WebRTC socket ready, waiting for peers...");

        // Loop principal: manejar eventos de peers y mensajes
        loop {
            // Procesar eventos de conexión/desconexión de peers
            for (peer_id, peer_state) in socket.update_peers() {
                match peer_state {
                    PeerState::Connected => {
                        println!("🔗 Peer connected: {:?}", peer_id);
                        // No asignamos player_id aquí, esperamos el mensaje JOIN
                    }
                    PeerState::Disconnected => {
                        println!("🔌 Peer disconnected: {:?}", peer_id);
                        let _ = event_tx.send(NetworkEvent::PlayerDisconnected { peer_id });
                    }
                }
            }

            // Recibir mensajes del canal 0 (reliable - control)
            for (peer_id, packet) in socket.channel_mut(0).receive() {
                if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                    // Manejar mensaje y obtener posible respuesta
                    if let Some(response) = handle_control_message_typed(&event_tx, &state, peer_id, msg) {
                        // Enviar respuesta al cliente (ej: VersionMismatch)
                        if let Ok(data) = bincode::serialize(&response) {
                            socket.channel_mut(0).send(data.into(), peer_id);
                        }
                    }
                }
            }

            // Recibir mensajes del canal 1 (unreliable - game data)
            for (peer_id, packet) in socket.channel_mut(1).receive() {
                if let Ok(msg) = bincode::deserialize::<GameDataMessage>(&packet) {
                    handle_game_data_message_typed(&event_tx, peer_id, msg);
                }
            }

            // Enviar mensajes salientes desde Bevy a los clientes
            while let Ok(outgoing) = outgoing_rx.try_recv() {
                match outgoing {
                    OutgoingMessage::ToOne {
                        peer_id,
                        channel,
                        data,
                    } => {
                        socket.channel_mut(channel).send(data.into(), peer_id);
                    }
                    OutgoingMessage::Broadcast { channel, data } => {
                        // Colectar peer_ids primero para evitar borrow conflict
                        let peers: Vec<_> = socket.connected_peers().collect();
                        for peer_id in peers {
                            socket
                                .channel_mut(channel)
                                .send(data.clone().into(), peer_id);
                        }
                    }
                }
            }

            // Pequeña pausa para no saturar el CPU
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    });
}

/// Maneja un mensaje de control y devuelve una respuesta opcional para enviar al cliente
pub fn handle_control_message_typed(
    event_tx: &mpsc::Sender<NetworkEvent>,
    state: &Arc<Mutex<NetworkState>>,
    peer_id: PeerId,
    msg: ControlMessage,
) -> Option<ControlMessage> {
    match msg {
        ControlMessage::Join { player_name, client_version } => {
            // Verificar versión del cliente
            let (min_version, id) = {
                let mut s = state.lock().unwrap();

                // Obtener versión mínima
                let min_version = s.min_client_version;

                // Verificar si el cliente tiene una versión compatible
                if let Some(cv) = client_version {
                    if !cv.is_compatible_with(&min_version) {
                        println!(
                            "❌ Cliente rechazado: versión {} es menor que la mínima {}",
                            cv, min_version
                        );
                        return Some(ControlMessage::VersionMismatch {
                            client_version: cv,
                            min_required: min_version,
                            message: "Por favor actualiza tu cliente a la última versión.".to_string(),
                        });
                    }
                    println!("✅ Cliente versión {} aceptado (mínima: {})", cv, min_version);
                } else {
                    // Cliente antiguo sin versión - podrías rechazarlo o aceptarlo
                    println!("⚠️  Cliente sin versión (legacy), aceptando...");
                }

                let id = s.next_player_id;
                s.next_player_id += 1;
                (min_version, id)
            };

            println!("🎮 Player {} joined: {}", id, player_name);

            let _ = event_tx.send(NetworkEvent::NewPlayer {
                id,
                name: player_name,
                peer_id,
            });

            None // El Welcome se envía desde process_network_messages
        }
        ControlMessage::Ready => {
            println!("✅ Player with peer_id {:?} ready", peer_id);
            let _ = event_tx.send(NetworkEvent::PlayerReady { peer_id });
            None
        }
        ControlMessage::Leave { player_id } => {
            println!("👋 Player {} requested to leave", player_id);
            let _ = event_tx.send(NetworkEvent::PlayerLeave { player_id });
            None
        }
        ControlMessage::MovePlayer {
            player_id,
            team_index,
            is_starter,
        } => {
            println!(
                "🔄 MovePlayer request: player {} -> team {:?}, starter {:?}",
                player_id, team_index, is_starter
            );
            let _ = event_tx.send(NetworkEvent::MovePlayer {
                admin_peer_id: peer_id,
                player_id,
                team_index,
                is_starter,
            });
            None
        }
        ControlMessage::KickPlayer { player_id } => {
            println!("👢 KickPlayer request: player {}", player_id);
            let _ = event_tx.send(NetworkEvent::KickPlayer {
                admin_peer_id: peer_id,
                player_id,
            });
            None
        }
        ControlMessage::ToggleAdmin { player_id, is_admin } => {
            println!(
                "👑 ToggleAdmin request: player {} -> admin={}",
                player_id, is_admin
            );
            let _ = event_tx.send(NetworkEvent::ToggleAdmin {
                admin_peer_id: peer_id,
                player_id,
                is_admin,
            });
            None
        }
        _ => {
            // Otros mensajes de control del servidor no deberían venir del cliente
            None
        }
    }
}

pub fn handle_game_data_message_typed(
    event_tx: &mpsc::Sender<NetworkEvent>,
    _peer_id: PeerId,
    msg: GameDataMessage,
) {
    match msg {
        GameDataMessage::Input { player_id, input } => {
            // Ahora usamos el player_id del mensaje directamente
            let _ = event_tx.send(NetworkEvent::PlayerInputById { player_id, input });
        }
        GameDataMessage::Ping { timestamp } => {
            println!("Debería responder con Pong {}", timestamp);
        }
        _ => {
            // Otros mensajes del servidor no deberían venir del cliente
        }
    }
}

pub fn update_input_manager(mut game_input: ResMut<GameInputManager>) {
    game_input.tick();
}

pub fn process_network_messages(
    mut commands: Commands,
    network_rx: ResMut<NetworkReceiver>,
    network_tx: Res<NetworkSender>,
    config: Res<GameConfig>,
    loaded_map: Res<LoadedMap>,
    mut game_input: ResMut<GameInputManager>,
    mut players: Query<(&mut Player, Entity)>,
    mut match_slots: ResMut<HostMatchSlots>,
    mut sphere_query: Query<(&mut Transform, &mut Velocity, &mut CollisionGroups), With<Sphere>>,
) {
    let mut slots_changed = false;

    while let Ok(event) = network_rx.0.lock().unwrap().try_recv() {
        match event {
            NetworkEvent::NewPlayer { id, name, peer_id } => {
                // Agregar jugador al GameInputManager
                game_input.add_player(id);

                // Enviar WELCOME al nuevo jugador
                let welcome_msg = ControlMessage::Welcome {
                    player_id: id,
                    map: loaded_map.0.clone(),
                };

                if let Ok(data) = bincode::serialize(&welcome_msg) {
                    println!("📤 Enviando WELCOME a jugador {}", id);
                    let _ = network_tx.0.send(OutgoingMessage::ToOne {
                        peer_id,
                        channel: 0, // Canal reliable
                        data,
                    });
                }

                spawn_physics(&mut commands, id, name, peer_id, &config, &mut match_slots.0);
                slots_changed = true;

                // Send current slots state to the new player
                let slots_msg = ControlMessage::SlotsUpdated(match_slots.0.clone());
                if let Ok(data) = bincode::serialize(&slots_msg) {
                    let _ = network_tx.0.send(OutgoingMessage::ToOne {
                        peer_id,
                        channel: 0,
                        data,
                    });
                }
            }

            NetworkEvent::PlayerInput { peer_id, input } => {
                // Buscar el player_id real usando el peer_id (legacy, un jugador por peer)
                for (player, _) in players.iter() {
                    if player.peer_id == peer_id {
                        game_input.update_input(player.id, input.clone());
                        break;
                    }
                }
            }

            NetworkEvent::PlayerInputById { player_id, input } => {
                // Input identificado directamente por player_id (multijugador local)
                game_input.update_input(player_id, input);
            }

            NetworkEvent::PlayerDisconnected { peer_id } => {
                for (player, entity) in players.iter() {
                    if player.peer_id == peer_id {
                        // Remove from slots
                        match_slots.0.remove_player(player.id);
                        match_slots.0.admins.remove(&player.id);
                        slots_changed = true;

                        // Notificar a todos los clientes que este jugador se desconectó
                        let disconnect_msg = ControlMessage::PlayerDisconnected {
                            player_id: player.id,
                        };
                        if let Ok(data) = bincode::serialize(&disconnect_msg) {
                            let _ = network_tx.0.send(OutgoingMessage::Broadcast {
                                channel: 0, // Canal reliable
                                data,
                            });
                        }

                        // Despawnear tanto Player como Sphere con todos sus hijos
                        commands.entity(player.sphere).despawn();
                        commands.entity(entity).despawn();
                        // Remover del GameInputManager
                        game_input.remove_player(player.id);
                        println!(
                            "❌ Jugador {} ({}) desconectado y removido",
                            player.name, player.id
                        );
                        break;
                    }
                }
            }

            NetworkEvent::PlayerReady { peer_id } => {
                for (mut player, _) in players.iter_mut() {
                    if player.peer_id == peer_id {
                        player.is_ready = true;
                        println!(
                            "✅ Jugador {} marcado como READY en el loop de juego",
                            player.id
                        );
                        break;
                    }
                }
            }

            NetworkEvent::PlayerLeave { player_id } => {
                for (player, entity) in players.iter() {
                    if player.id == player_id {
                        // Remove from slots
                        match_slots.0.remove_player(player.id);
                        match_slots.0.admins.remove(&player.id);
                        slots_changed = true;

                        // Notificar a todos los clientes que este jugador se fue
                        let disconnect_msg = ControlMessage::PlayerDisconnected { player_id };
                        if let Ok(data) = bincode::serialize(&disconnect_msg) {
                            let _ = network_tx.0.send(OutgoingMessage::Broadcast {
                                channel: 0, // Canal reliable
                                data,
                            });
                        }

                        // Despawnear tanto Player como Sphere
                        commands.entity(player.sphere).despawn();
                        commands.entity(entity).despawn();
                        // Remover del GameInputManager
                        game_input.remove_player(player.id);
                        println!(
                            "👋 Jugador {} ({}) salió voluntariamente y fue removido",
                            player.name, player.id
                        );
                        break;
                    }
                }
            }

            NetworkEvent::MovePlayer {
                admin_peer_id,
                player_id,
                team_index,
                is_starter,
            } => {
                // Find admin's player_id from peer_id
                let admin_player_id = players
                    .iter()
                    .find(|(p, _)| p.peer_id == admin_peer_id)
                    .map(|(p, _)| p.id);

                // Verify admin has permission
                if let Some(admin_id) = admin_player_id {
                    if match_slots.0.is_admin(admin_id) {
                        // Check if player was a starter before
                        let was_starter = match_slots.0.is_starter(player_id);

                        // Move the player in slots
                        match_slots.0.move_player(player_id, team_index, is_starter);
                        slots_changed = true;

                        // Determine if now a starter
                        let now_starter = is_starter == Some(true);

                        // Find the player and update physics
                        for (mut player, _) in players.iter_mut() {
                            if player.id == player_id {
                                // Update team_index if moving to a team
                                if let Some(t_idx) = team_index {
                                    println!(
                                        "🔄 [Server] Jugador {} team_index: {} -> {}",
                                        player_id, player.team_index, t_idx
                                    );
                                    player.team_index = t_idx;
                                }

                                // Handle physics activation/deactivation
                                if let Ok((mut transform, mut velocity, mut collision_groups)) =
                                    sphere_query.get_mut(player.sphere)
                                {
                                    if was_starter && !now_starter {
                                        // Leaving field: move far away and disable collisions
                                        transform.translation.x = 99999.0;
                                        transform.translation.y = 99999.0;
                                        velocity.linvel = Vec2::ZERO;
                                        velocity.angvel = 0.0;
                                        // Disable all collisions
                                        collision_groups.memberships = Group::NONE;
                                        collision_groups.filters = Group::NONE;
                                        println!(
                                            "🚫 Jugador {} física desactivada (fuera del campo)",
                                            player_id
                                        );
                                    } else if !was_starter && now_starter {
                                        // Entering field: spawn at team position
                                        let spawn_x = if player.team_index == 0 {
                                            -500.0 - (player_id as f32 * 100.0)
                                        } else {
                                            500.0 + (player_id as f32 * 100.0)
                                        };
                                        transform.translation.x = spawn_x;
                                        transform.translation.y = 0.0;
                                        // Re-enable collisions (GROUP_4 = players)
                                        collision_groups.memberships = Group::GROUP_4;
                                        collision_groups.filters = Group::ALL ^ Group::GROUP_5;
                                        println!(
                                            "✅ Jugador {} física activada (en el campo)",
                                            player_id
                                        );
                                    }
                                }

                                println!(
                                    "🔄 Jugador {} movido: team={:?}, starter={:?}",
                                    player_id, team_index, is_starter
                                );
                                break;
                            }
                        }
                    } else {
                        println!(
                            "⚠️ Player {} tried to move player but is not admin",
                            admin_id
                        );
                    }
                }
            }

            NetworkEvent::KickPlayer {
                admin_peer_id,
                player_id,
            } => {
                // Find admin's player_id from peer_id
                let admin_player_id = players
                    .iter()
                    .find(|(p, _)| p.peer_id == admin_peer_id)
                    .map(|(p, _)| p.id);

                // Verify admin has permission
                if let Some(admin_id) = admin_player_id {
                    if match_slots.0.is_admin(admin_id) {
                        // Find and kick the player
                        for (player, entity) in players.iter() {
                            if player.id == player_id {
                                // Remove from slots
                                match_slots.0.remove_player(player.id);
                                match_slots.0.admins.remove(&player.id);
                                slots_changed = true;

                                // Notify all clients
                                let disconnect_msg =
                                    ControlMessage::PlayerDisconnected { player_id };
                                if let Ok(data) = bincode::serialize(&disconnect_msg) {
                                    let _ = network_tx.0.send(OutgoingMessage::Broadcast {
                                        channel: 0,
                                        data,
                                    });
                                }

                                // Despawn
                                commands.entity(player.sphere).despawn();
                                commands.entity(entity).despawn();
                                game_input.remove_player(player.id);
                                println!("👢 Jugador {} expulsado por admin {}", player_id, admin_id);
                                break;
                            }
                        }
                    } else {
                        println!(
                            "⚠️ Player {} tried to kick player but is not admin",
                            admin_id
                        );
                    }
                }
            }

            NetworkEvent::ToggleAdmin {
                admin_peer_id,
                player_id,
                is_admin,
            } => {
                // Find admin's player_id from peer_id
                let admin_player_id = players
                    .iter()
                    .find(|(p, _)| p.peer_id == admin_peer_id)
                    .map(|(p, _)| p.id);

                // Verify sender is admin
                if let Some(admin_id) = admin_player_id {
                    if match_slots.0.is_admin(admin_id) {
                        if is_admin {
                            match_slots.0.add_admin(player_id);
                            println!("👑 Jugador {} ahora es admin (otorgado por {})", player_id, admin_id);
                        } else {
                            match_slots.0.remove_admin(player_id);
                            println!("👑 Jugador {} ya no es admin (removido por {})", player_id, admin_id);
                        }
                        slots_changed = true;
                    } else {
                        println!(
                            "⚠️ Player {} tried to toggle admin but is not admin",
                            admin_id
                        );
                    }
                }
            }
        }
    }

    // Broadcast slots update if changed
    if slots_changed {
        let slots_msg = ControlMessage::SlotsUpdated(match_slots.0.clone());
        if let Ok(data) = bincode::serialize(&slots_msg) {
            let _ = network_tx.0.send(OutgoingMessage::Broadcast {
                channel: 0,
                data,
            });
        }
    }
}

pub fn broadcast_game_state(
    time: Res<Time>,
    mut broadcast_timer: ResMut<BroadcastTimer>,
    mut tick: ResMut<GameTick>,
    players: Query<&Player>,
    sphere_query: Query<(&Transform, &Velocity), With<Sphere>>,
    ball: Query<(&Transform, &Velocity, &Ball), Without<Sphere>>,
    network_tx: Res<NetworkSender>,
    match_slots: Res<HostMatchSlots>,
) {
    // Actualizar timer
    broadcast_timer.0.tick(time.delta());

    // Solo enviar cuando el timer se completa (30 veces por segundo)
    if !broadcast_timer.0.just_finished() {
        return;
    }

    tick.0 += 1;

    // Construir estado - only include starters (players on field with physics)
    let player_states: Vec<PlayerState> = players
        .iter()
        .filter_map(|player| {
            // Only include starters in game state
            if !match_slots.0.is_starter(player.id) {
                return None;
            }

            if let Ok((transform, velocity)) = sphere_query.get(player.sphere) {
                // Extraer rotación Z del quaternion
                let (_, _, rotation_z) = transform.rotation.to_euler(EulerRot::XYZ);

                Some(PlayerState {
                    id: player.id,
                    name: player.name.clone(),
                    position: Vec2::new(transform.translation.x, transform.translation.y),
                    velocity: (velocity.linvel.x, velocity.linvel.y),
                    rotation: rotation_z,
                    kick_charge: player.kick_charge,
                    kick_charging: player.kick_charging,
                    is_sliding: player.is_sliding,
                    not_interacting: player.not_interacting,
                    ball_target_position: player.ball_target_position,
                    stamin_charge: player.stamin,
                    active_movement: player.active_movement.clone(),
                    team_index: player.team_index,
                    mode_cube_active: player.mode_cube_active,
                })
            } else {
                println!(
                    "⚠️  No se pudo obtener Transform/Velocity para jugador {}",
                    player.id
                );
                None
            }
        })
        .collect();

    // Log cada 60 ticks (2 segundos)
    if tick.0 % 60 == 0 {
        println!(
            "📊 [Tick {}] Jugadores: {}, Ready: {}",
            tick.0,
            players.iter().count(),
            players.iter().filter(|p| p.is_ready).count()
        );
    }

    let ball_state = if let Ok((transform, velocity, ball)) = ball.single() {
        BallState {
            position: (transform.translation.x, transform.translation.y),
            velocity: (velocity.linvel.x, velocity.linvel.y),
            angular_velocity: ball.angular_velocity,
        }
    } else {
        BallState {
            position: (0.0, 0.0),
            velocity: (0.0, 0.0),
            angular_velocity: 0.0,
        }
    };

    let game_state = ServerMessage::GameState {
        tick: tick.0,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        players: player_states,
        ball: ball_state,
    };

    // Extraer datos de game_state (ServerMessage) para GameDataMessage
    let (tick_num, timestamp_num, players_vec, ball_state_data) = match game_state {
        ServerMessage::GameState {
            tick,
            timestamp,
            players,
            ball,
        } => (tick, timestamp, players, ball),
        _ => return, // No debería pasar
    };

    // Crear GameDataMessage para el canal unreliable
    let game_data_msg = GameDataMessage::GameState {
        tick: tick_num,
        timestamp: timestamp_num,
        players: players_vec,
        ball: ball_state_data,
    };

    // Serializar y broadcast a todos los jugadores READY
    if let Ok(data) = bincode::serialize(&game_data_msg) {
        // Enviar solo a jugadores que están ready
        for player in players.iter() {
            if player.is_ready {
                let _ = network_tx.0.send(OutgoingMessage::ToOne {
                    peer_id: player.peer_id,
                    channel: 1, // Canal unreliable para GameState
                    data: data.clone(),
                });
            }
        }
    }
}

// ============================================================================
// PROXY REGISTRATION
// ============================================================================

#[derive(serde::Serialize)]
struct CreateRoomRequest {
    room_id: String,
    name: String,
    max_players: u8,
    map_name: Option<String>,
    min_version: Option<String>,
}

#[derive(serde::Deserialize)]
struct CreateRoomResponse {
    token: String,
}

async fn register_room_with_proxy(
    http_url: &str,
    room_id: &str,
    room_name: &str,
    max_players: u8,
    map_name: Option<&str>,
    min_version: Option<&str>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/rooms", http_url);

    let request = CreateRoomRequest {
        room_id: room_id.to_string(),
        name: room_name.to_string(),
        max_players,
        map_name: map_name.map(|s| s.to_string()),
        min_version: min_version.map(|s| s.to_string()),
    };

    println!(
        "📡 Registering room '{}' with proxy at {}",
        room_id, http_url
    );

    let mut req = client.post(&url).json(&request);
    for (key, value) in hmac_auth::auth_headers() {
        req = req.header(key, value);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status().is_success() {
        let body: CreateRoomResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        Ok(body.token)
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Proxy returned error {}: {}", status, body))
    }
}
