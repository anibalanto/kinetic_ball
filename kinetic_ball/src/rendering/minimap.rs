use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::sprite_render::ColorMaterial;

use crate::components::{
    InGameEntity, MinimapDot, MinimapFieldLine, MinimapPlayerName, RemoteBall, RemotePlayer,
};
use crate::rendering::field::approximate_curve_for_rendering;
use crate::resources::PlayerColors;
use crate::shared::map::Map;
use crate::shared::protocol::GameConfig;

// Constantes para el minimapa
const MINIMAP_LINE_Z: f32 = 0.0;

// Crea sprites para las líneas del minimapa (layer 1)
pub fn spawn_minimap_lines(commands: &mut Commands, map: &Map) {
    let line_color = Color::srgba(1.0, 1.0, 1.0, 0.7);

    // Calcular grosor proporcional al tamaño del mapa
    // Para que las líneas se vean de ~3px en un minimapa de 300px
    let map_width = map.width.or(map.bg.width).unwrap_or(1000.0);
    let line_thickness = map_width / 200.0; // ~0.5% del ancho del mapa

    // Dibujar segmentos visibles
    for segment in &map.segments {
        if !segment.is_visible() {
            continue;
        }

        if segment.v0 >= map.vertexes.len() || segment.v1 >= map.vertexes.len() {
            continue;
        }

        let v0 = &map.vertexes[segment.v0];
        let v1 = &map.vertexes[segment.v1];

        let p0 = Vec2::new(v0.x, v0.y);
        let p1 = Vec2::new(v1.x, v1.y);

        let curve_factor = segment.curve.or(segment.curve_f).unwrap_or(0.0);

        if curve_factor.abs() < 0.01 {
            // Segmento recto
            spawn_minimap_line_segment(commands, p0, p1, line_color, line_thickness);
        } else {
            // Segmento curvo - aproximar con múltiples líneas
            let points = approximate_curve_for_rendering(p0, p1, curve_factor, 16);
            for i in 0..points.len() - 1 {
                spawn_minimap_line_segment(
                    commands,
                    points[i],
                    points[i + 1],
                    line_color,
                    line_thickness,
                );
            }
        }
    }
}

// Crea un sprite rectangular para el minimapa (layer 1)
pub fn spawn_minimap_line_segment(
    commands: &mut Commands,
    p0: Vec2,
    p1: Vec2,
    color: Color,
    thickness: f32,
) {
    let delta = p1 - p0;
    let length = delta.length();
    if length < 0.01 {
        return;
    }

    let midpoint = (p0 + p1) * 0.5;
    let angle = delta.y.atan2(delta.x);

    commands.spawn((
        InGameEntity,
        Sprite {
            color,
            custom_size: Some(Vec2::new(length, thickness)),
            ..default()
        },
        Transform::from_xyz(midpoint.x, midpoint.y, MINIMAP_LINE_Z)
            .with_rotation(Quat::from_rotation_z(angle)),
        MinimapFieldLine,
        RenderLayers::layer(1),
    ));
}

/// Crea puntos y nombres en Layer 1 cuando aparecen jugadores/pelota
pub fn spawn_minimap_dots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    config: Res<GameConfig>,
    player_colors: Res<PlayerColors>,
    existing_dots: Query<&MinimapDot>,
    existing_names: Query<&MinimapPlayerName>,
    players_with_dots: Query<(Entity, &RemotePlayer)>,
    ball_with_dots: Query<Entity, With<RemoteBall>>,
) {
    // Crear set de entidades ya trackeadas por dots
    let tracked_dots: std::collections::HashSet<Entity> =
        existing_dots.iter().map(|dot| dot.tracks_entity).collect();

    // Crear set de entidades ya trackeadas por nombres
    let tracked_names: std::collections::HashSet<Entity> = existing_names
        .iter()
        .map(|name| name.tracks_entity)
        .collect();

    // Spawn dots y nombres para jugadores que aún no tienen
    for (entity, player) in players_with_dots.iter() {
        // Spawn dot si no existe
        if !tracked_dots.contains(&entity) {
            // Color del equipo desde config
            let team_color = config
                .team_colors
                .get(player.team_index as usize)
                .copied()
                .unwrap_or((0.5, 0.5, 0.5));

            let dot_color = Color::srgb(team_color.0, team_color.1, team_color.2);

            // Círculo de 120px para jugadores
            commands.spawn((
                InGameEntity,
                Mesh2d(meshes.add(Circle::new(120.0))),
                MeshMaterial2d(materials.add(dot_color)),
                Transform::from_xyz(0.0, 0.0, 10.0),
                MinimapDot {
                    tracks_entity: entity,
                },
                RenderLayers::layer(1),
            ));
        }

        // Spawn nombre si no existe
        if !tracked_names.contains(&entity) {
            // Usar color único del jugador para el nombre en el minimapa
            let name_color = player_colors
                .colors
                .get(&player.id)
                .copied()
                .unwrap_or(Color::WHITE);

            // Crear un nodo de texto para el nombre del jugador
            commands.spawn((
                InGameEntity,
                Text2d::new(player.name.clone()),
                TextFont {
                    font_size: 80.0,
                    ..default()
                },
                TextColor(name_color),
                Transform::from_xyz(0.0, 150.0, 12.0), // Posición encima del dot
                MinimapPlayerName {
                    tracks_entity: entity,
                },
                RenderLayers::layer(1),
            ));
        }
    }

    // Spawn dot para pelota si no tiene
    for entity in ball_with_dots.iter() {
        if tracked_dots.contains(&entity) {
            continue;
        }

        // Círculo de 80px blanco para pelota
        commands.spawn((
            InGameEntity,
            Mesh2d(meshes.add(Circle::new(80.0))),
            MeshMaterial2d(materials.add(Color::WHITE)),
            Transform::from_xyz(0.0, 0.0, 11.0),
            MinimapDot {
                tracks_entity: entity,
            },
            RenderLayers::layer(1),
        ));
    }
}

/// Sincroniza posición de puntos con entidades reales
pub fn sync_minimap_dots(
    mut dots: Query<(&MinimapDot, &mut Transform)>,
    transforms: Query<&Transform, Without<MinimapDot>>,
) {
    for (dot, mut dot_transform) in dots.iter_mut() {
        if let Ok(tracked_transform) = transforms.get(dot.tracks_entity) {
            dot_transform.translation.x = tracked_transform.translation.x;
            dot_transform.translation.y = tracked_transform.translation.y;
        }
    }
}

/// Elimina puntos cuando desaparecen entidades
pub fn cleanup_minimap_dots(
    mut commands: Commands,
    dots: Query<(Entity, &MinimapDot)>,
    names: Query<(Entity, &MinimapPlayerName)>,
    entities: Query<Entity>,
) {
    // Limpiar dots
    for (dot_entity, dot) in dots.iter() {
        // Si la entidad trackeada ya no existe, eliminar el dot
        if entities.get(dot.tracks_entity).is_err() {
            commands.entity(dot_entity).despawn();
        }
    }

    // Limpiar nombres
    for (name_entity, name) in names.iter() {
        if entities.get(name.tracks_entity).is_err() {
            commands.entity(name_entity).despawn();
        }
    }
}

/// Sincroniza posición de nombres del minimapa con entidades reales
pub fn sync_minimap_names(
    mut names: Query<(&MinimapPlayerName, &mut Transform), Without<MinimapDot>>,
    transforms: Query<&Transform, (Without<MinimapPlayerName>, Without<MinimapDot>)>,
) {
    for (name, mut name_transform) in names.iter_mut() {
        if let Ok(tracked_transform) = transforms.get(name.tracks_entity) {
            name_transform.translation.x = tracked_transform.translation.x;
            name_transform.translation.y = tracked_transform.translation.y + 150.0;
            // Encima del dot
        }
    }
}
