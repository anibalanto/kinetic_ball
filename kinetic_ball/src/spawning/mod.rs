use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::sprite_render::ColorMaterial;

use crate::assets::EmbeddedAssets;
use crate::color_utils::{generate_unique_player_color, get_team_colors};
use crate::components::{
    CurveAction, Interpolated, KickChargeBar, KickChargeBarCurveLeft, KickChargeBarCurveRight,
    PlayerNameText, PlayerOutline, PlayerSprite, RemoteBall, RemotePlayer, SlideCubeVisual,
    StaminChargeBar,
};
use crate::events::{SpawnBallEvent, SpawnPlayerEvent};
use crate::game::spawn_key_visual_2d;
use crate::keybindings::{key_code_display_name, GamepadBindingsMap, KeyBindingsConfig};
use crate::local_players::{InputDevice, LocalPlayers};
use crate::resources::PlayerColors;
use crate::shared::protocol::GameConfig;

/// Handler para eventos de spawn de pelota
pub fn handle_spawn_ball(
    mut events: MessageReader<SpawnBallEvent>,
    mut commands: Commands,
    embedded_assets: Res<EmbeddedAssets>,
    config: Res<GameConfig>,
) {
    for event in events.read() {
        println!("⚽ [Bevy] Spawneando pelota visual en {:?}", event.position);

        commands
            .spawn((
                Transform::from_xyz(event.position.0, event.position.1, 10.0),
                Visibility::default(),
                RemoteBall,
                bevy_rapier2d::prelude::Collider::ball(config.ball_radius),
                Interpolated {
                    target_position: Vec2::new(event.position.0, event.position.1),
                    target_velocity: Vec2::new(event.velocity.0, event.velocity.1),
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
}

/// Handler para eventos de spawn de jugador
pub fn handle_spawn_player(
    mut events: MessageReader<SpawnPlayerEvent>,
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    keybindings: Res<KeyBindingsConfig>,
    gamepad_bindings_map: Res<GamepadBindingsMap>,
    local_players: Res<LocalPlayers>,
    mut player_colors: ResMut<PlayerColors>,
) {
    for event in events.read() {
        let ps = &event.player_state;
        let is_local = event.is_local;

        let public_player_layers = if is_local {
            RenderLayers::from_layers(&[0, 2])
        } else {
            RenderLayers::layer(0)
        };

        let private_player_layers = if is_local {
            RenderLayers::from_layers(&[0, 2])
        } else {
            RenderLayers::none()
        };

        let preview_player_layers = if is_local {
            RenderLayers::layer(2)
        } else {
            RenderLayers::none()
        };

        println!(
            "🆕 [Bevy] Spawneando jugador visual: {} (ID: {}) {}",
            ps.name,
            ps.id,
            if is_local { "(LOCAL)" } else { "" }
        );

        // Colores de equipo desde la configuracion
        let (player_color, opposite_color) = get_team_colors(ps.team_index, &config.team_colors);

        // Generar color unico para el jugador (para nombre y minimapa)
        let unique_player_color = if let Some(&color) = player_colors.colors.get(&ps.id) {
            color
        } else {
            let color = generate_unique_player_color(&mut player_colors);
            player_colors.colors.insert(ps.id, color);
            color
        };

        commands
            .spawn((
                Transform::from_xyz(ps.position.x, ps.position.y, 10.0),
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
                bevy_rapier2d::prelude::Collider::ball(config.sphere_radius),
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

                // Circulo de borde (outline) - negro, ligeramente mas grande
                parent.spawn((
                    Mesh2d(meshes.add(Circle::new(radius + outline_thickness))),
                    MeshMaterial2d(materials.add(Color::BLACK)),
                    Transform::from_xyz(0.0, 0.0, 0.5),
                    PlayerOutline,
                    public_player_layers.clone(),
                ));

                // Circulo principal (relleno) - color del jugador
                parent.spawn((
                    Mesh2d(meshes.add(Circle::new(radius))),
                    MeshMaterial2d(materials.add(player_color)),
                    Transform::from_xyz(0.0, 0.0, 1.0),
                    PlayerSprite { parent_id: ps.id },
                    public_player_layers.clone(),
                ));

                // Indicador de direccion (cubo pequeno hacia adelante)
                let indicator_size = radius / 3.0;
                let indicator_x = radius * 0.7;
                let indicator_y = 0.0;
                let cube_scale = 1.0;

                // Mesh personalizado: cuadrado con 4 vertices (LineStrip)
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
                    preview_player_layers.clone(),
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
                let local_player_opt = local_players
                    .players
                    .iter()
                    .find(|lp| lp.server_player_id == Some(ps.id));

                let (curve_left_text, curve_right_text) =
                    if let Some(local_player) = local_player_opt {
                        match &local_player.input_device {
                            InputDevice::RawGamepad(_) => {
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
                            _ => (
                                key_code_display_name(keybindings.curve_left.0),
                                key_code_display_name(keybindings.curve_right.0),
                            ),
                        }
                    } else {
                        (
                            key_code_display_name(keybindings.curve_left.0),
                            key_code_display_name(keybindings.curve_right.0),
                        )
                    };

                let angle_90 = std::f32::consts::FRAC_PI_2;

                // Tecla/boton izquierda (curve_left)
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

                // Tecla/boton derecha (curve_right)
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
