use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::sprite_render::ColorMaterial;

use crate::assets::EmbeddedAssets;
use crate::color_utils::{generate_unique_player_color, get_team_colors};
use crate::components::{
    CurveAction, Interpolated, KickChargeBar, KickChargeBarCurveLeft, KickChargeBarCurveRight,
    PlayerCamera, PlayerNameText, PlayerOutline, PlayerSprite, RemoteBall, RemotePlayer,
    SlideCubeVisual, StaminChargeBar,
};
use crate::game::spawn_key_visual_2d;
use crate::keybindings::{key_code_display_name, GamepadBindingsMap, KeyBindingsConfig};
use crate::local_players::{InputDevice, LocalPlayers};
use crate::resources::{GameTick, LoadedMap, NetworkChannels, PlayerColors};
use crate::shared::protocol::{GameConfig, ServerMessage};

#[derive(SystemParam)]
pub struct NetworkParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub embedded_assets: Res<'w, EmbeddedAssets>,
    pub config: ResMut<'w, GameConfig>,
    pub channels: Res<'w, NetworkChannels>,
    pub my_id: ResMut<'w, crate::resources::MyPlayerId>,
    pub loaded_map: ResMut<'w, LoadedMap>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<ColorMaterial>>,
    pub keybindings: Res<'w, KeyBindingsConfig>,
    pub gamepad_bindings_map: Res<'w, GamepadBindingsMap>,
    pub local_players: ResMut<'w, LocalPlayers>,
    pub game_tick: ResMut<'w, GameTick>,
    pub player_colors: ResMut<'w, PlayerColors>,
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
    // --- Re-asignación para mantener compatibilidad con tu lógica actual ---
    let commands = &mut params.commands;
    let embedded_assets = &params.embedded_assets;
    let config = &mut params.config;
    let channels = &params.channels;
    let my_id = &mut params.my_id;
    let loaded_map = &mut params.loaded_map;
    let mut meshes = &mut params.meshes;
    let mut materials = &mut params.materials;
    let keybindings = &params.keybindings;
    let local_players = &mut params.local_players;
    let game_tick = &mut params.game_tick;
    let gamepad_bindings_map = params.gamepad_bindings_map;
    let player_colors = &mut params.player_colors;

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

    // Procesar solo el último GameState si hay múltiples (incluye tick)
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
                            "   Asociado a jugador local '{}' (índice {})",
                            local_player.name, local_player.local_index
                        );
                    }
                }

                // my_id guarda el primer player_id (para compatibilidad y cámara)
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
                            "   Vértices: {}, Segmentos: {}, Discos: {}",
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
            _ => {}
        }
    }

    // Procesar solo el último GameState si existe
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
            println!("⚽ [Bevy] Spawneando pelota visual en {:?}", ball.position);
            // Usar textura con children
            commands
                .spawn((
                    Transform::from_xyz(ball.position.0, ball.position.1, 10.0), // Z=10 para estar sobre las líneas del mapa
                    Visibility::default(),
                    RemoteBall,
                    bevy_rapier2d::prelude::Collider::ball(config.ball_radius), // Para debug rendering
                    Interpolated {
                        target_position: Vec2::new(ball.position.0, ball.position.1),
                        target_velocity: Vec2::new(ball.velocity.0, ball.velocity.1),
                        target_rotation: 0.0,
                        smoothing: 20.0,
                    },
                    RenderLayers::layer(0),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Sprite {
                            image: embedded_assets.ball_texture.clone(),
                            custom_size: Some(Vec2::splat(config.ball_radius * 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(0.0, 0.0, 1.0),
                        RenderLayers::from_layers(&[0, 2]),
                    ));
                });
        }

        // Actualizar Jugadores
        for ps in players {
            let mut found = false;
            for (_entity, mut interp, mut transform, mut rp, _children) in players_q.iter_mut() {
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

                    found = true;
                    break;
                }
            }
            if !found && !spawned_this_frame.contains(&ps.id) {
                spawned_this_frame.insert(ps.id);

                // Determinar si es un jugador local (cualquiera de los jugadores locales)
                let is_local = local_players
                    .players
                    .iter()
                    .any(|lp| lp.server_player_id == Some(ps.id));

                let public_player_layers = if is_local {
                    RenderLayers::from_layers(&[0, 2]) // Visible en cámara principal y detalle
                } else {
                    RenderLayers::layer(0) // Solo visible en cámara principal
                };

                let private_player_layers = if is_local {
                    RenderLayers::from_layers(&[0, 2]) // Visible en cámara principal y detalle
                } else {
                    RenderLayers::none() // No visible
                };

                let preview_player_layers = if is_local {
                    RenderLayers::layer(2) // Visible en cámara de detalle
                } else {
                    RenderLayers::none() // No visible
                };

                println!(
                    "🆕 [Bevy] Spawneando jugador visual: {} (ID: {}) {}",
                    ps.name,
                    ps.id,
                    if is_local { "(LOCAL)" } else { "" }
                );

                // Colores de equipo desde la configuración
                let (player_color, opposite_color) =
                    get_team_colors(ps.team_index, &config.team_colors);

                // Generar color único para el jugador (para nombre y minimapa)
                let unique_player_color =
                    player_colors
                        .colors
                        .get(&ps.id)
                        .copied()
                        .unwrap_or_else(|| {
                            let color = generate_unique_player_color(player_colors);
                            player_colors.colors.insert(ps.id, color);
                            color
                        });

                // Usar textura con children
                commands
                    .spawn((
                        Transform::from_xyz(ps.position.x, ps.position.y, 10.0), // Z=10 para estar sobre las líneas del mapa
                        Visibility::default(),
                        RemotePlayer {
                            id: ps.id,
                            name: ps.name.clone(),
                            team_index: ps.team_index,
                            kick_charge: ps.kick_charge,
                            is_sliding: ps.is_sliding,
                            not_interacting: ps.not_interacting,
                            base_color: player_color,
                            ball_target_position: ps.ball_target_position,
                            stamin_charge: ps.stamin_charge,
                            active_movement: ps.active_movement.clone(),
                            mode_cube_active: ps.mode_cube_active,
                        },
                        bevy_rapier2d::prelude::Collider::ball(config.sphere_radius), // Para debug rendering
                        Interpolated {
                            target_position: ps.position,
                            target_velocity: Vec2::new(ps.velocity.0, ps.velocity.1),
                            target_rotation: ps.rotation,
                            smoothing: 15.0,
                        },
                        public_player_layers.clone(),
                    ))
                    .with_children(|parent| {
                        let radius = config.sphere_radius;
                        let outline_thickness = 3.0;

                        // Círculo de borde (outline) - negro, ligeramente más grande
                        parent.spawn((
                            Mesh2d(meshes.add(Circle::new(radius + outline_thickness))),
                            MeshMaterial2d(materials.add(Color::BLACK)),
                            Transform::from_xyz(0.0, 0.0, 0.5),
                            PlayerOutline,
                            public_player_layers.clone(),
                        ));

                        // Círculo principal (relleno) - color del jugador
                        parent.spawn((
                            Mesh2d(meshes.add(Circle::new(radius))),
                            MeshMaterial2d(materials.add(player_color)),
                            Transform::from_xyz(0.0, 0.0, 1.0),
                            PlayerSprite { parent_id: ps.id },
                            public_player_layers.clone(),
                        ));

                        // Indicador de dirección (cubo pequeño hacia adelante)
                        let indicator_size = radius / 3.0;
                        // Posición fija hacia adelante del jugador
                        let indicator_x = radius * 0.7;
                        let indicator_y = 0.0;
                        let cube_scale = 1.0; // Escala inicial, modificable por movimientos

                        // Mesh personalizado: cuadrado con 4 vértices (LineStrip)
                        let half = indicator_size / 2.0;
                        let mut square_mesh = Mesh::new(
                            bevy::mesh::PrimitiveTopology::LineStrip,
                            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
                        );
                        square_mesh.insert_attribute(
                            Mesh::ATTRIBUTE_POSITION,
                            vec![
                                [-half, -half, 0.0],
                                [half, -half, 0.0],
                                [half, half, 0.0],
                                [-half, half, 0.0],
                                [-half, -half, 0.0],
                            ],
                        );

                        parent.spawn((
                            Mesh2d(meshes.add(square_mesh)),
                            MeshMaterial2d(materials.add(Color::WHITE)),
                            Transform::from_xyz(indicator_x, indicator_y, 1.5)
                                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
                                .with_scale(Vec3::splat(cube_scale)),
                            SlideCubeVisual { parent_id: ps.id },
                            preview_player_layers.clone(),
                        ));

                        // Barra de carga de patada
                        parent.spawn((
                            KickChargeBar,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(0.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform::from_xyz(0.0, 0.0, 30.0),
                            private_player_layers.clone(),
                        ));

                        let angle = 25.0f32.to_radians();

                        // Barra de carga de patada a la izquierda
                        parent.spawn((
                            KickChargeBarCurveLeft,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(5.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(0.0, 10.0, 30.0),
                                // Rotación hacia la izquierda (positiva en el eje Z)
                                rotation: Quat::from_rotation_z(angle),
                                ..default()
                            },
                            private_player_layers.clone(),
                        ));

                        // Barra de carga de patada a la derecha
                        parent.spawn((
                            KickChargeBarCurveRight,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(5.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(0.0, -10.0, 30.0),
                                // Rotación hacia la derecha (negativa en el eje Z)
                                rotation: Quat::from_rotation_z(-angle),
                                ..default()
                            },
                            private_player_layers.clone(),
                        ));

                        let angle_90 = 90.0f32.to_radians();

                        // Solo para jugador local: barra de estamina
                        parent.spawn((
                            StaminChargeBar,
                            Sprite {
                                color: opposite_color,
                                custom_size: Some(Vec2::new(0.0, 5.0)),
                                ..default()
                            },
                            Anchor::CENTER_LEFT,
                            Transform {
                                translation: Vec3::new(-10.0, -15.0, 30.0),
                                rotation: Quat::from_rotation_z(angle_90),
                                ..default()
                            },
                            preview_player_layers,
                        ));

                        // Nombre del jugador debajo del sprite
                        parent.spawn((
                            PlayerNameText,
                            Text2d::new(ps.name.clone()),
                            TextFont {
                                font_size: 20.0,
                                ..default()
                            },
                            TextColor(unique_player_color),
                            Transform::from_xyz(-config.sphere_radius * 1.5, 0.0, 10.0),
                            RenderLayers::layer(0),
                        ));

                        // Indicadores de teclas/botones de curva (solo para jugador local)
                        // Buscar el jugador local correspondiente para saber qué tipo de input usa
                        let local_player_opt = local_players
                            .players
                            .iter()
                            .find(|lp| lp.server_player_id == Some(ps.id));

                        let (curve_left_text, curve_right_text) =
                            if let Some(local_player) = local_player_opt {
                                match &local_player.input_device {
                                    InputDevice::RawGamepad(_) => {
                                        // Usar bindings del gamepad
                                        let gamepad_bindings = local_player
                                            .gamepad_type_name
                                            .as_ref()
                                            .map(|name| gamepad_bindings_map.get_bindings(name))
                                            .unwrap_or_default();

                                        let left = gamepad_bindings
                                            .curve_left
                                            .map(|b| b.display_name())
                                            .unwrap_or_else(|| "?".to_string());
                                        let right = gamepad_bindings
                                            .curve_right
                                            .map(|b| b.display_name())
                                            .unwrap_or_else(|| "?".to_string());
                                        (left, right)
                                    }
                                    _ => {
                                        // Usar bindings del teclado
                                        (
                                            key_code_display_name(keybindings.curve_left.0),
                                            key_code_display_name(keybindings.curve_right.0),
                                        )
                                    }
                                }
                            } else {
                                // Fallback a teclado
                                (
                                    key_code_display_name(keybindings.curve_left.0),
                                    key_code_display_name(keybindings.curve_right.0),
                                )
                            };

                        let angle_90 = std::f32::consts::FRAC_PI_2;

                        // Tecla/botón izquierda (curve_left)
                        spawn_key_visual_2d(
                            parent,
                            &curve_left_text,
                            ps.id,
                            CurveAction::Left,
                            Vec3::new(
                                config.sphere_radius / 2.0,
                                -config.sphere_radius * 2.0,
                                50.0,
                            ),
                            Quat::from_rotation_z(-angle_90),
                            &mut meshes,
                            &mut materials,
                            private_player_layers.clone(),
                        );

                        // Tecla/botón derecha (curve_right)
                        spawn_key_visual_2d(
                            parent,
                            &curve_right_text,
                            ps.id,
                            CurveAction::Right,
                            Vec3::new(config.sphere_radius / 2.0, config.sphere_radius * 2.0, 50.0),
                            Quat::from_rotation_z(-angle_90),
                            &mut meshes,
                            &mut materials,
                            private_player_layers.clone(),
                        );
                    });
            }
        }
    }
}
