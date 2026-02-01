use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, TextureDimension, TextureFormat, TextureUsages};
use bevy::sprite_render::ColorMaterial;
use bevy::ui::widget::ViewportNode;
use bevy::ui::UiTargetCamera;
use bevy_egui::PrimaryEguiContext;
use std::f32::consts::FRAC_PI_2;

use crate::components::{
    CompositorCamera, CurveAction, DefaultFieldLine, FieldBackground, GameUiCamera, InGameEntity,
    KeyVisual, MinimapCamera, PlayerCamera, PlayerDetailCamera, SplitScreenQuad,
};
use crate::local_players::LocalPlayers;
use crate::resources::{DynamicSplitState, SplitScreenMaterial, SplitScreenTextures};
use crate::shared::protocol::GameConfig;

pub fn spawn_key_visual_2d(
    parent: &mut ChildSpawnerCommands,
    key_text: &str,
    player_id: u32,
    action: CurveAction,
    translation: Vec3,
    rotation: Quat,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    render_layers: RenderLayers,
) {
    let font_size = match key_text.len() {
        1 => 32.0,
        2 => 28.0,
        _ => 22.0,
    };

    let key_width = (key_text.len() as f32 * 20.0).max(50.0);
    let key_height = 50.0;

    parent
        .spawn((
            KeyVisual { player_id, action },
            Transform {
                translation,
                rotation, // <--- Aplicamos la rotación aquí
                ..default()
            },
            Visibility::default(),
            render_layers.clone(),
        ))
        .with_children(|key| {
            // SOMBRA (Fija en la base)
            key.spawn((
                Mesh2d(meshes.add(Rectangle::new(key_width + 4.0, key_height + 8.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.05, 0.05, 0.05))),
                Transform::from_xyz(0.0, -4.0, -0.1),
                render_layers.clone(),
            ));

            // CUERPO MÓVIL (Lo que se hunde al presionar)
            key.spawn((
                Transform::default(),
                Visibility::default(),
                render_layers.clone(),
            ))
            .with_children(|body| {
                // Borde/Highlight
                body.spawn((
                    Mesh2d(meshes.add(Rectangle::new(key_width + 2.0, key_height + 2.0))),
                    MeshMaterial2d(materials.add(Color::srgb(0.4, 0.4, 0.4))),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                    render_layers.clone(),
                ));

                // Superficie principal
                body.spawn((
                    Mesh2d(meshes.add(Rectangle::new(key_width, key_height))),
                    MeshMaterial2d(materials.add(Color::srgb(0.2, 0.2, 0.2))),
                    Transform::from_xyz(0.0, 0.0, 0.2),
                    render_layers.clone(),
                ));

                // Texto de la tecla
                body.spawn((
                    Text2d::new(key_text),
                    TextFont {
                        font_size,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Transform::from_xyz(0.0, 0.0, 0.3),
                    render_layers.clone(),
                ));
            });
        });
}

pub fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut split_materials: ResMut<Assets<SplitScreenMaterial>>,
    mut split_textures: ResMut<SplitScreenTextures>,
    local_players: Res<LocalPlayers>,
    windows: Query<&Window>,
) {
    // Obtener tamaño de ventana para calcular viewports
    let window = windows.iter().next();
    // Tamaño físico (para texturas de render target)
    let window_size = window
        .map(|w| UVec2::new(w.physical_width(), w.physical_height()))
        .unwrap_or(UVec2::new(1280, 720));
    // Tamaño lógico (para el quad del compositor, porque ScalingMode::WindowSize usa unidades lógicas)
    let window_logical_size = window
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));

    // Número de jugadores locales (mínimo 1)
    let num_local_players = local_players.players.len().max(1);
    let use_dynamic_split = num_local_players >= 2;

    // Para split-screen dinámico: crear texturas de render target para cada cámara
    let mut player_camera_textures: Vec<Handle<Image>> = Vec::new();

    if use_dynamic_split {
        for i in 0..2 {
            let mut cam_image = Image::new_uninit(
                bevy::render::render_resource::Extent3d {
                    width: window_size.x,
                    height: window_size.y,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Bgra8UnormSrgb,
                RenderAssetUsages::all(),
            );
            cam_image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT;
            let cam_image_handle = images.add(cam_image);
            player_camera_textures.push(cam_image_handle);
        }

        // Guardar referencias a las texturas
        split_textures.camera1_texture = player_camera_textures.first().cloned();
        split_textures.camera2_texture = player_camera_textures.get(1).cloned();
    }

    // Crear cámaras para cada jugador local
    let mut player_cameras: Vec<Entity> = Vec::new();
    for i in 0..num_local_players.min(2) {
        let server_player_id = local_players
            .players
            .get(i)
            .and_then(|lp| lp.server_player_id);

        if use_dynamic_split {
            // Renderizar a textura para composición dinámica
            let target_texture = player_camera_textures.get(i).cloned();
            if let Some(texture) = target_texture {
                let cam = commands
                    .spawn((
                        InGameEntity,
                        Camera2d,
                        Camera {
                            order: -(10 + i as isize), // Renderizar antes del compositor
                            target: RenderTarget::Image(texture.into()),
                            clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.15)),
                            ..default()
                        },
                        Projection::Orthographic(OrthographicProjection {
                            scale: 3.0,
                            ..OrthographicProjection::default_2d()
                        }),
                        Transform::from_xyz(0.0, 0.0, 999.0),
                        PlayerCamera {
                            local_index: i as u8,
                            server_player_id,
                        },
                        RenderLayers::layer(0),
                    ))
                    .id();
                player_cameras.push(cam);
            }
        } else {
            // Un solo jugador: renderizar directamente a pantalla
            commands.spawn((
                InGameEntity,
                Camera2d,
                Camera {
                    order: i as isize,
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scale: 3.0,
                    ..OrthographicProjection::default_2d()
                }),
                Transform::from_xyz(0.0, 0.0, 999.0),
                PlayerCamera {
                    local_index: i as u8,
                    server_player_id,
                },
                RenderLayers::layer(0),
            ));
        }
    }

    // Crear compositor de split-screen si hay 2+ jugadores
    if use_dynamic_split {
        if let (Some(tex1), Some(tex2)) = (
            player_camera_textures.first().cloned(),
            player_camera_textures.get(1).cloned(),
        ) {
            // Crear el material de composición
            let split_material = split_materials.add(SplitScreenMaterial {
                camera1_texture: tex1,
                camera2_texture: tex2,
                split_params: Vec4::new(FRAC_PI_2, 0.0, 0.5, 0.5), // angle, factor, center_x, center_y
            });

            // Crear cámara compositor con proyección fija al tamaño de ventana
            commands.spawn((
                InGameEntity,
                Camera2d,
                Camera {
                    order: 0, // Después de las cámaras de jugador, antes de UI
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: window_logical_size.x / 2.0,
                        height: window_logical_size.y / 2.0,
                    },
                    ..OrthographicProjection::default_2d()
                }),
                CompositorCamera,
                RenderLayers::layer(3), // Layer especial para compositor
            ));

            // Quad fullscreen que muestra la composición
            commands.spawn((
                InGameEntity,
                Mesh2d(meshes.add(Rectangle::new(window_logical_size.x, window_logical_size.y))),
                MeshMaterial2d(split_material),
                Transform::from_xyz(0.0, 0.0, 0.0),
                SplitScreenQuad,
                RenderLayers::layer(3),
            ));
        }
    }

    // --- Crear texturas de render target para ViewportNodes ---

    let minimap_size = config.minimap_size;
    let detail_size = config.player_detail_size;

    // Textura para minimapa
    let mut minimap_image = Image::new_uninit(
        bevy::render::render_resource::Extent3d {
            width: minimap_size.x,
            height: minimap_size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::all(),
    );
    minimap_image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let minimap_image_handle = images.add(minimap_image);

    // --- Cámara minimapa - Layer 1 (renderiza a textura) ---
    let minimap_camera = commands
        .spawn((
            InGameEntity,
            Camera2d,
            Camera {
                order: -2, // Renderiza antes que la principal
                target: RenderTarget::Image(minimap_image_handle.clone().into()),
                clear_color: ClearColorConfig::Custom(Color::srgba(0.1, 0.3, 0.1, 0.6)),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: config.arena_width,
                    height: config.arena_height,
                },
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_xyz(0.0, 0.0, 999.0),
            MinimapCamera,
            RenderLayers::layer(1),
        ))
        .id();

    // --- Crear cámaras de detalle para cada jugador local ---
    let mut detail_cameras: Vec<Entity> = Vec::new();
    for i in 0..num_local_players.min(2) {
        // Crear textura de detalle para este jugador
        let mut detail_image = Image::new_uninit(
            bevy::render::render_resource::Extent3d {
                width: detail_size.x,
                height: detail_size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Bgra8UnormSrgb,
            RenderAssetUsages::all(),
        );
        detail_image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::RENDER_ATTACHMENT;
        let detail_image_handle = images.add(detail_image);

        let detail_camera = commands
            .spawn((
                InGameEntity,
                Camera2d,
                Camera {
                    order: -1 - i as isize, // Renderiza antes que la principal
                    target: RenderTarget::Image(detail_image_handle.clone().into()),
                    clear_color: ClearColorConfig::Custom(Color::srgba(0.2, 0.2, 0.2, 0.6)),
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scale: 1.5,
                    ..OrthographicProjection::default_2d()
                }),
                Transform::from_xyz(0.0, 0.0, 999.0),
                PlayerDetailCamera {
                    local_index: i as u8,
                },
                RenderLayers::layer(2),
            ))
            .id();

        detail_cameras.push(detail_camera);
    }

    // --- Cámara UI dedicada (sin viewport, renderiza UI en pantalla completa) ---
    let ui_camera = commands
        .spawn((
            InGameEntity,
            Camera2d,
            Camera {
                order: 100, // Renderiza después de todo lo demás
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(BlendState::ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                },
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            GameUiCamera,
            // Marcar como contexto primario para egui
            PrimaryEguiContext,
            // No renderiza nada del juego, solo UI
            RenderLayers::none(),
        ))
        .id();

    // La línea divisoria se dibuja directamente en el shader del compositor

    // --- UI con ViewportNodes ---
    // Contenedor raíz para posicionar los viewports
    commands
        .spawn((
            InGameEntity,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                padding: UiRect::all(Val::Px(config.ui_padding)),
                ..default()
            },
            UiTargetCamera(ui_camera),
            // No bloquear clicks en el juego
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // Detalle del jugador 1 (izquierda abajo) - circular
            if let Some(&detail_cam) = detail_cameras.first() {
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BorderRadius::all(Val::Percent(50.0)), // Circular
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                    ViewportNode::new(detail_cam),
                ));
            } else {
                // Espaciador si no hay jugadores
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }

            // Minimapa (centro abajo)
            parent.spawn((
                Node {
                    width: Val::Px(minimap_size.x as f32),
                    height: Val::Px(minimap_size.y as f32),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BorderColor::all(Color::WHITE),
                BorderRadius::all(Val::Px(4.0)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                ViewportNode::new(minimap_camera),
            ));

            // Detalle del jugador 2 (derecha abajo) - circular
            if let Some(&detail_cam) = detail_cameras.get(1) {
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BorderRadius::all(Val::Percent(50.0)), // Circular
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                    ViewportNode::new(detail_cam),
                ));
            } else if num_local_players == 1 {
                // Espaciador para mantener el minimapa centrado cuando hay 1 jugador
                parent.spawn((
                    Node {
                        width: Val::Px(detail_size.x as f32),
                        height: Val::Px(detail_size.y as f32),
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }
        });

    // El Campo de Juego (Césped) - Color verde - Layer 0
    commands.spawn((
        InGameEntity,
        Sprite {
            color: Color::srgb(0.2, 0.5, 0.2),
            custom_size: Some(Vec2::new(config.arena_width, config.arena_height)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0), // Z = 0 (fondo)
        FieldBackground,
        RenderLayers::layer(0),
    ));

    // Las líneas del minimapa se crean dinámicamente cuando se carga el mapa
    // Líneas blancas del campo (bordes)
    let thickness = 5.0;
    let w = config.arena_width;
    let h = config.arena_height;

    // Top
    commands.spawn((
        InGameEntity,
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(w + thickness, thickness)),
            ..default()
        },
        Transform::from_xyz(0.0, h / 2.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Bottom
    commands.spawn((
        InGameEntity,
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(w + thickness, thickness)),
            ..default()
        },
        Transform::from_xyz(0.0, -h / 2.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Left
    commands.spawn((
        InGameEntity,
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(thickness, h + thickness)),
            ..default()
        },
        Transform::from_xyz(-w / 2.0, 0.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // Right
    commands.spawn((
        InGameEntity,
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(thickness, h + thickness)),
            ..default()
        },
        Transform::from_xyz(w / 2.0, 0.0, 0.0),
        DefaultFieldLine,
        RenderLayers::layer(0),
    ));

    // El mensaje Ready ahora se envía automáticamente en el thread de red después de recibir Welcome

    println!("✅ Cliente configurado y campo listo");
}

/// Limpia todas las entidades del juego al salir de InGame
pub fn cleanup_game(
    mut commands: Commands,
    entities: Query<Entity, With<InGameEntity>>,
    mut local_players: ResMut<crate::local_players::LocalPlayers>,
    mut network_channels: ResMut<crate::resources::NetworkChannels>,
    mut loaded_map: ResMut<crate::resources::LoadedMap>,
) {
    println!("🧹 Limpiando entidades del juego...");

    let count = entities.iter().count();
    for entity in entities.iter() {
        commands.entity(entity).despawn();
    }

    // Resetear server_player_id (el servidor asignará nuevos al reconectar)
    for player in &mut local_players.players {
        player.server_player_id = None;
    }

    // Limpiar canales de red (el hilo ya terminó)
    network_channels.receiver = None;
    network_channels.sender = None;
    network_channels.control_sender = None;

    // Resetear mapa cargado (para que is_changed() detecte el nuevo al reconectar)
    loaded_map.0 = None;

    println!("✅ {} entidades del juego limpiadas", count);
}
