use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::sprite_render::ColorMaterial;

use crate::color_utils::get_team_colors;
use crate::components::{
    Interpolated, KickChargeBar, KickChargeBarCurveLeft, KickChargeBarCurveRight, PlayerCamera,
    PlayerSprite, RemoteBall, RemotePlayer,
};
use crate::events::{SpawnBallEvent, SpawnPlayerEvent};
use crate::local_players::LocalPlayers;
use crate::resources::{AdminPanelState, ClientMatchSlots, GameTick, LoadedMap, NetworkChannels};
use crate::shared::protocol::{GameConfig, ServerMessage};

#[derive(SystemParam)]
pub struct NetworkParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub config: ResMut<'w, GameConfig>,
    pub channels: Res<'w, NetworkChannels>,
    pub my_id: ResMut<'w, crate::resources::MyPlayerId>,
    pub loaded_map: ResMut<'w, LoadedMap>,
    pub materials: ResMut<'w, Assets<ColorMaterial>>,
    pub local_players: ResMut<'w, LocalPlayers>,
    pub game_tick: ResMut<'w, GameTick>,
    pub spawn_ball_events: MessageWriter<'w, SpawnBallEvent>,
    pub spawn_player_events: MessageWriter<'w, SpawnPlayerEvent>,
    pub match_slots: ResMut<'w, ClientMatchSlots>,
    pub admin_state: ResMut<'w, AdminPanelState>,
}

#[derive(SystemParam)]
pub struct NetworkQueries<'w, 's> {
    pub ball_q: Query<
        'w,
        's,
        (
            &'static mut Interpolated,
            &'static mut Transform,
            &'static RemoteBall,
        ),
        Without<RemotePlayer>,
    >,
    pub players_q: Query<
        'w,
        's,
        (
            Entity,
            &'static mut Interpolated,
            &'static mut Transform,
            &'static mut RemotePlayer,
            &'static Children,
        ),
        (Without<RemoteBall>, Without<PlayerCamera>),
    >,
    pub bar_sprites: Query<
        'w,
        's,
        &'static mut Sprite,
        Or<(With<KickChargeBarCurveLeft>, With<KickChargeBarCurveRight>)>,
    >,
    pub player_materials: Query<
        'w,
        's,
        (
            &'static PlayerSprite,
            &'static MeshMaterial2d<ColorMaterial>,
        ),
    >,
    pub children_query: Query<'w, 's, &'static Children>,
    pub text_color_query: Query<'w, 's, &'static mut TextColor>,
}

pub fn process_network_messages(mut params: NetworkParams, mut queries: NetworkQueries) {
    let commands = &mut params.commands;
    let config = &mut params.config;
    let channels = &params.channels;
    let my_id = &mut params.my_id;
    let loaded_map = &mut params.loaded_map;
    let materials = &mut params.materials;
    let local_players = &mut params.local_players;
    let game_tick = &mut params.game_tick;
    let spawn_ball_events = &mut params.spawn_ball_events;
    let spawn_player_events = &mut params.spawn_player_events;
    let match_slots = &mut params.match_slots;
    let admin_state = &mut params.admin_state;

    let ball_q = &mut queries.ball_q;
    let players_q = &mut queries.players_q;
    let bar_sprites = &mut queries.bar_sprites;
    let player_materials = &queries.player_materials;
    let children_query = &queries.children_query;
    let text_color_query = &mut queries.text_color_query;

    let Some(ref receiver) = channels.receiver else {
        return;
    };
    let rx = receiver.lock().unwrap();
    let mut spawned_this_frame = std::collections::HashSet::new();

    // Procesar solo el ultimo GameState si hay multiples (incluye tick)
    let mut last_game_state: Option<(
        u32, // tick
        Vec<crate::shared::protocol::PlayerState>,
        crate::shared::protocol::BallState,
    )> = None;
    let mut messages = Vec::new();

    while let Ok(msg) = rx.try_recv() {
        messages.push(msg);
    }

    for msg in messages {
        match msg {
            ServerMessage::Welcome { player_id, map } => {
                println!("🎉 [Bevy] Welcome recibido. PlayerID: {}", player_id);

                // Asociar este player_id con el siguiente jugador local sin ID asignado
                if !local_players.is_empty() {
                    // Buscar el primer jugador local sin server_player_id asignado
                    if let Some(local_player) = local_players
                        .players
                        .iter_mut()
                        .find(|p| p.server_player_id.is_none())
                    {
                        local_player.server_player_id = Some(player_id);
                        println!(
                            "   Asociado a jugador local '{}' (indice {})",
                            local_player.name, local_player.local_index
                        );
                    }
                }

                // my_id guarda el primer player_id (para compatibilidad y camara)
                if my_id.0.is_none() {
                    my_id.0 = Some(player_id);
                }

                // Almacenar mapa si fue enviado (solo del primer Welcome)
                if loaded_map.0.is_none() {
                    if let Some(received_map) = map {
                        println!("📦 [Bevy] Mapa recibido: {}", received_map.name);
                        println!(
                            "   Dimensiones: width={:?}, height={:?}",
                            received_map.width, received_map.height
                        );
                        println!(
                            "   BG: width={:?}, height={:?}",
                            received_map.bg.width, received_map.bg.height
                        );
                        println!(
                            "   Vertices: {}, Segmentos: {}, Discos: {}",
                            received_map.vertexes.len(),
                            received_map.segments.len(),
                            received_map.discs.len()
                        );
                        loaded_map.0 = Some(received_map);
                    } else {
                        println!("🏟️  [Bevy] Usando arena por defecto");
                    }
                }
            }
            ServerMessage::GameState {
                players,
                ball,
                tick,
                ..
            } => {
                // Log solo el primer GameState recibido
                if tick == 1 {
                    println!("📥 [Bevy] Primer GameState recibido: {} jugadores, pelota en ({:.0}, {:.0})",
                        players.len(), ball.position.0, ball.position.1);
                }
                last_game_state = Some((tick, players, ball));
            }
            ServerMessage::ChangeTeamColor { team_index, color } => {
                println!(
                    "🎨 Cambio de color equipo {}: ({:.2}, {:.2}, {:.2})",
                    team_index, color.0, color.1, color.2
                );

                // 1. Actualizar config
                while config.team_colors.len() <= team_index as usize {
                    config.team_colors.push((0.5, 0.5, 0.5));
                }
                config.team_colors[team_index as usize] = color;

                // 2. Calcular nuevos colores
                let (player_color, opposite_color) =
                    get_team_colors(team_index, &config.team_colors);

                // 3. Actualizar jugadores de ese equipo
                for (_, _, _, player, children) in players_q.iter() {
                    if player.team_index != team_index {
                        continue;
                    }

                    for child in children.iter() {
                        // Actualizar sprite del jugador
                        if let Ok((_, mat_handle)) = player_materials.get(child) {
                            if let Some(mat) = materials.get_mut(&mat_handle.0) {
                                mat.color = player_color;
                            }
                        }

                        // Actualizar barras de carga
                        if let Ok(mut sprite) = bar_sprites.get_mut(child) {
                            sprite.color = opposite_color;

                            // Actualizar texto hijo de la barra
                            if let Ok(bar_children) = children_query.get(child) {
                                for text_entity in bar_children.iter() {
                                    if let Ok(mut text_color) =
                                        text_color_query.get_mut(text_entity)
                                    {
                                        text_color.0 = opposite_color;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ServerMessage::PlayerDisconnected { player_id } => {
                // Buscar y eliminar el jugador desconectado
                for (entity, _, _, rp, _) in players_q.iter() {
                    if rp.id == player_id {
                        commands.entity(entity).despawn();
                        println!("👋 [Bevy] Jugador {} eliminado del juego", player_id);
                        break;
                    }
                }
            }
            ServerMessage::SlotsUpdated(slots) => {
                println!(
                    "📊 [Bevy] Slots actualizados - Spectators: {}, Team0: {}/{}, Team1: {}/{}",
                    slots.spectators.len(),
                    slots.teams[0].starters.len(),
                    slots.teams[0].substitutes.len(),
                    slots.teams[1].starters.len(),
                    slots.teams[1].substitutes.len()
                );

                // Update local slots copy
                match_slots.0 = slots.clone();

                // Update admin status for local players
                if let Some(player_id) = my_id.0 {
                    admin_state.is_admin = slots.is_admin(player_id);
                    if admin_state.is_admin {
                        println!("👑 [Bevy] Eres administrador de la sala");
                    }
                }
            }
            _ => {}
        }
    }

    // Procesar solo el ultimo GameState si existe
    if let Some((tick, players, ball)) = last_game_state {
        game_tick.0 = tick;

        // Actualizar Pelota
        let ball_exists = !ball_q.is_empty();
        if ball_exists {
            for (mut interp, mut transform, _) in ball_q.iter_mut() {
                interp.target_position = Vec2::new(ball.position.0, ball.position.1);
                interp.target_velocity = Vec2::new(ball.velocity.0, ball.velocity.1);
                transform.translation.x = ball.position.0;
                transform.translation.y = ball.position.1;
            }
        } else {
            // Emitir evento para spawn de pelota
            spawn_ball_events.write(SpawnBallEvent {
                position: ball.position,
                velocity: ball.velocity,
            });
        }

        // Primero: detectar jugadores que cambiaron de equipo y despawnearlos
        // Esto fuerza un respawn con los colores correctos
        let mut team_changed_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut entities_to_despawn: Vec<Entity> = Vec::new();

        for ps in &players {
            for (entity, _, _, rp, _) in players_q.iter() {
                if rp.id == ps.id && rp.team_index != ps.team_index {
                    println!(
                        "🔄 [Client] Jugador {} cambió de equipo {} -> {}, despawneando para respawn",
                        rp.id, rp.team_index, ps.team_index
                    );
                    team_changed_ids.insert(ps.id);
                    entities_to_despawn.push(entity);
                    break;
                }
            }
        }

        // Despawnear jugadores que cambiaron de equipo
        for entity in entities_to_despawn {
            commands.entity(entity).despawn();
        }

        // Actualizar Jugadores
        for ps in players {
            // Si cambió de equipo, ya fue despawneado, forzar respawn
            if team_changed_ids.contains(&ps.id) {
                if !spawned_this_frame.contains(&ps.id) {
                    spawned_this_frame.insert(ps.id);

                    let is_local = local_players
                        .players
                        .iter()
                        .any(|lp| lp.server_player_id == Some(ps.id));

                    println!(
                        "🎨 [Client] Respawneando jugador {} con equipo {}",
                        ps.id, ps.team_index
                    );

                    spawn_player_events.write(SpawnPlayerEvent {
                        player_state: ps,
                        is_local,
                    });
                }
                continue;
            }

            let mut found = false;
            for (_entity, mut interp, mut transform, mut rp, _) in players_q.iter_mut() {
                if rp.id == ps.id {
                    interp.target_position = ps.position;
                    interp.target_velocity = Vec2::new(ps.velocity.0, ps.velocity.1);
                    interp.target_rotation = ps.rotation;
                    transform.translation.x = ps.position.x;
                    transform.translation.y = ps.position.y;
                    rp.kick_charge = ps.kick_charge;
                    rp.is_sliding = ps.is_sliding;
                    rp.ball_target_position = ps.ball_target_position;
                    rp.stamin_charge = ps.stamin_charge;
                    rp.active_movement = ps.active_movement.clone();
                    rp.mode_cube_active = ps.mode_cube_active;
                    rp.team_index = ps.team_index;

                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);

                // Determinar si es un jugador local
                let is_local = local_players
                    .players
                    .iter()
                    .any(|lp| lp.server_player_id == Some(ps.id));

                // Emitir evento para spawn de jugador
                spawn_player_events.write(SpawnPlayerEvent {
                    player_state: ps,
                    is_local,
                });
            }
        }
    }

    // Process slots: despawn non-starters
    // Only process if we have received slots from server (not empty)
    let slots = &match_slots.0;
    let has_any_slots = !slots.teams[0].starters.is_empty()
        || !slots.teams[1].starters.is_empty()
        || !slots.spectators.is_empty()
        || !slots.teams[0].substitutes.is_empty()
        || !slots.teams[1].substitutes.is_empty();

    if has_any_slots {
        let all_starters: std::collections::HashSet<u32> = slots
            .teams
            .iter()
            .flat_map(|t| t.starters.iter().copied())
            .collect();

        // Collect entities to despawn (can't despawn while iterating)
        let mut to_despawn: Vec<Entity> = Vec::new();

        for (entity, _, _, rp, _) in players_q.iter() {
            if !all_starters.contains(&rp.id) {
                to_despawn.push(entity);
                println!("🚫 [Bevy] Despawneando jugador {} (no es starter)", rp.id);
            }
        }

        // Despawn non-starters
        for entity in to_despawn {
            commands.entity(entity).despawn();
        }
    }
}
