use bevy::prelude::*;

use crate::components::RemotePlayer;
use crate::keybindings::{GamepadBindingsMap, GilrsWrapper, KeyBindingsConfig};
use crate::local_players::{read_local_player_input, LocalPlayers};
use crate::resources::{MyPlayerId, NetworkChannels};

/// Sistema que lee input de todos los jugadores locales y lo envía al servidor
pub fn handle_multi_player_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    channels: Res<NetworkChannels>,
    local_players: Res<LocalPlayers>,
    my_player_id: Res<MyPlayerId>,
    gilrs: Option<Res<GilrsWrapper>>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    keybindings: Res<KeyBindingsConfig>,
    players: Query<&RemotePlayer>,
) {
    let Some(ref sender) = channels.sender else {
        return;
    };

    // Si no hay jugadores locales configurados, usar modo legacy (un jugador con teclado)
    if local_players.is_empty() {
        let Some(my_id) = my_player_id.0 else {
            return;
        };

        // Verificar si el jugador local está en modo cubo
        let is_cube_mode = players
            .iter()
            .find(|p| p.id == my_id)
            .map(|p| p.mode_cube_active)
            .unwrap_or(false);

        // Leer input del teclado (modo legacy)
        let input = crate::local_players::read_keyboard_input(&keyboard, &keybindings, is_cube_mode);

        // Enviar input con el player_id
        if let Err(e) = sender.send((my_id, input)) {
            println!("⚠️ [Bevy] Error enviando input al canal: {:?}", e);
        }
        return;
    }

    // DEBUG: Log una vez cada 60 frames aproximadamente
    static mut FRAME_COUNT: u32 = 0;
    unsafe {
        FRAME_COUNT += 1;
        if FRAME_COUNT % 120 == 0 {
            println!(
                "🎮 [DEBUG] {} jugadores locales configurados, gamepads en query: {}",
                local_players.players.len(),
                gamepads.iter().count()
            );
            for (i, p) in local_players.players.iter().enumerate() {
                println!(
                    "   Jugador {}: '{}', device={:?}, server_id={:?}",
                    i, p.name, p.input_device, p.server_player_id
                );
            }
        }
    }

    // Iterar sobre cada jugador local y enviar su input
    for local_player in &local_players.players {
        // Solo procesar si tiene un server_player_id asignado
        let Some(server_id) = local_player.server_player_id else {
            continue;
        };

        // Verificar si este jugador está en modo cubo
        let is_cube_mode = players
            .iter()
            .find(|p| p.id == server_id)
            .map(|p| p.mode_cube_active)
            .unwrap_or(false);

        // Leer input según el dispositivo asignado
        let input = read_local_player_input(
            local_player,
            &keyboard,
            &keybindings,
            &gamepads,
            gilrs.as_deref(),
            &gamepad_bindings_map,
            is_cube_mode,
        );

        // Enviar input con el player_id del servidor
        if let Err(e) = sender.send((server_id, input)) {
            println!(
                "⚠️ [Bevy] Error enviando input para jugador {} al canal: {:?}",
                server_id, e
            );
        }
    }
}
