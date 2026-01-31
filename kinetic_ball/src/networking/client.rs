use bevy::prelude::*;
use matchbox_socket::WebRtcSocket;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::local_players::LocalPlayers;
use crate::resources::{ConnectionConfig, NetworkChannels};
use crate::shared::protocol::{
    ControlMessage, GameDataMessage, PlayerInput, ProtocolVersion, ServerMessage,
};
use crate::states::AppState;

pub fn start_connection(
    config: Res<ConnectionConfig>,
    mut channels: ResMut<NetworkChannels>,
    local_players: Res<LocalPlayers>,
) {
    let (network_tx, network_rx) = mpsc::channel();
    let (input_tx, input_rx) = mpsc::channel();

    // Guardar los canales
    channels.receiver = Some(Arc::new(Mutex::new(network_rx)));
    channels.sender = Some(input_tx);

    let ws_url = config.ws_url();
    let room = config.room.clone();

    // Recoger los nombres de los jugadores locales
    // Si no hay jugadores locales configurados, usar el nombre del config (modo legacy)
    let player_names: Vec<String> = if local_players.is_empty() {
        vec![config.player_name.clone()]
    } else {
        local_players
            .players
            .iter()
            .map(|p| p.name.clone())
            .collect()
    };

    println!(
        "🌐 [Red] Iniciando conexión con {} jugadores locales",
        player_names.len()
    );

    // Iniciar hilo de red
    std::thread::spawn(move || {
        println!("🌐 [Red] Iniciando cliente WebRTC");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Fallo al crear Runtime de Tokio");

        rt.block_on(async {
            start_webrtc_client(ws_url, room, player_names, network_tx, input_rx).await;
        });
        println!("🌐 [Red] El hilo de red HA TERMINADO");
    });
}

pub fn check_connection(channels: Res<NetworkChannels>, mut next_state: ResMut<NextState<AppState>>) {
    // Verificar si hemos recibido el WELCOME
    if let Some(ref receiver) = channels.receiver {
        if let Ok(rx) = receiver.lock() {
            // Peek sin consumir - si hay mensajes, la conexión está lista
            // En realidad, simplemente pasamos a InGame y dejamos que process_network_messages maneje los mensajes
            drop(rx);
            // Por simplicidad, pasamos directamente a InGame después de un frame
            next_state.set(AppState::InGame);
        }
    }
}

pub async fn start_webrtc_client(
    server_url: String,
    room: String,
    player_names: Vec<String>,
    network_tx: mpsc::Sender<ServerMessage>,
    input_rx: mpsc::Receiver<(u32, PlayerInput)>,
) {
    // Conectar al proxy
    let room_url = format!("{}/{}", server_url, room);
    println!("🔌 [Red] Conectando a {}", room_url);

    // Crear WebRtcSocket y conectar a la room
    let (mut socket, loop_fut) = WebRtcSocket::builder(room_url)
        .add_channel(matchbox_socket::ChannelConfig::reliable()) // Canal 0: Control
        .add_channel(matchbox_socket::ChannelConfig::unreliable()) // Canal 1: GameData
        .build();

    // Spawn el loop de matchbox
    tokio::spawn(loop_fut);

    println!(
        "✅ [Red] WebRTC socket creado, esperando conexión con peers... ({} jugadores locales)",
        player_names.len()
    );

    // El server_peer_id real se determina cuando recibimos WELCOME
    let mut server_peer_id: Option<matchbox_socket::PeerId> = None;

    // Rastrear a qué peers ya enviamos JOINs
    let mut peers_joined: std::collections::HashSet<matchbox_socket::PeerId> =
        std::collections::HashSet::new();

    // Contador de WELCOMEs recibidos para asociar con local_index
    let mut welcomes_received: usize = 0;

    // Loop principal: recibir mensajes y enviar inputs
    loop {
        // Procesar nuevos peers y enviar JOINs para todos los jugadores locales
        socket.update_peers();
        let current_peers: Vec<_> = socket.connected_peers().collect();

        for peer_id in current_peers {
            if !peers_joined.contains(&peer_id) {
                // Nuevo peer, enviar JOIN para cada jugador local
                for (idx, name) in player_names.iter().enumerate() {
                    let client_version = ProtocolVersion::current();
                    let join_msg = ControlMessage::Join {
                        player_name: name.clone(),
                        client_version: Some(client_version),
                    };
                    if let Ok(data) = bincode::serialize(&join_msg) {
                        println!(
                            "📤 [Red] Enviando JOIN #{} ({}) v{} a peer {:?}...",
                            idx + 1,
                            name,
                            client_version,
                            peer_id
                        );
                        socket.channel_mut(0).send(data.into(), peer_id);
                    }
                }
                peers_joined.insert(peer_id);
            }
        }

        // Recibir mensajes del servidor
        // Canal 0: Control messages (reliable)
        for (peer_id, packet) in socket.channel_mut(0).receive() {
            if let Ok(msg) = bincode::deserialize::<ControlMessage>(&packet) {
                match msg {
                    ControlMessage::Welcome { player_id, map } => {
                        println!(
                            "🎉 [Red] WELCOME #{} recibido de peer {:?}! Player ID: {}",
                            welcomes_received + 1,
                            peer_id,
                            player_id
                        );

                        // Guardar el peer_id del servidor real (del primer WELCOME)
                        if server_peer_id.is_none() {
                            server_peer_id = Some(peer_id);
                        }

                        // Convertir a ServerMessage para compatibilidad con el código existente
                        let server_msg = ServerMessage::Welcome { player_id, map };
                        let _ = network_tx.send(server_msg);

                        // Enviar READY al servidor real
                        let ready_msg = ControlMessage::Ready;
                        if let Ok(data) = bincode::serialize(&ready_msg) {
                            println!(
                                "📤 [Red -> Servidor] Enviando READY para jugador {}...",
                                player_id
                            );
                            socket.channel_mut(0).send(data.into(), peer_id);
                        }

                        welcomes_received += 1;
                    }
                    ControlMessage::PlayerDisconnected { player_id } => {
                        println!("👋 [Red] Jugador {} se desconectó", player_id);
                        let _ = network_tx.send(ServerMessage::PlayerDisconnected { player_id });
                    }
                    ControlMessage::VersionMismatch {
                        client_version,
                        min_required,
                        message,
                    } => {
                        println!(
                            "❌ [Red] VERSION INCOMPATIBLE: Tu versión {} es menor que la mínima requerida {}",
                            client_version, min_required
                        );
                        println!("   {}", message);
                        let _ = network_tx.send(ServerMessage::Error {
                            message: format!(
                                "Versión incompatible: tienes v{}, se requiere v{} o superior. {}",
                                client_version, min_required, message
                            ),
                        });
                    }
                    ControlMessage::Error { message } => {
                        println!("❌ [Red] Error del servidor: {}", message);
                        let _ = network_tx.send(ServerMessage::Error { message });
                    }
                    _ => {}
                }
            }
        }

        // Canal 1: GameData messages (unreliable)
        for (_peer_id, packet) in socket.channel_mut(1).receive() {
            if let Ok(msg) = bincode::deserialize::<GameDataMessage>(&packet) {
                match msg {
                    GameDataMessage::GameState {
                        tick,
                        timestamp,
                        players,
                        ball,
                    } => {
                        // Convertir a ServerMessage
                        let server_msg = ServerMessage::GameState {
                            tick,
                            timestamp,
                            players,
                            ball,
                        };
                        let _ = network_tx.send(server_msg);
                    }
                    GameDataMessage::Pong {
                        client_timestamp,
                        server_timestamp,
                    } => {
                        let server_msg = ServerMessage::Pong {
                            client_timestamp,
                            server_timestamp,
                        };
                        let _ = network_tx.send(server_msg);
                    }
                    _ => {}
                }
            }
        }

        // Enviar inputs desde Bevy (solo si ya identificamos al servidor)
        if let Some(server_id) = server_peer_id {
            while let Ok((player_id, input)) = input_rx.try_recv() {
                let input_msg = GameDataMessage::Input { player_id, input };
                if let Ok(data) = bincode::serialize(&input_msg) {
                    socket.channel_mut(1).send(data.into(), server_id); // Canal 1 = unreliable
                }
            }
        } else {
            // Descartar inputs hasta que tengamos servidor
            while input_rx.try_recv().is_ok() {}
        }

        // Pequeña pausa
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }
}
