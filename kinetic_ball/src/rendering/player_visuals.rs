use bevy::prelude::*;
use bevy::sprite_render::ColorMaterial;

use crate::components::{
    KickChargeBar, KickChargeBarCurveLeft, KickChargeBarCurveRight, PlayerNameText, PlayerOutline,
    PlayerSprite, RemotePlayer, SlideCubeVisual, StaminChargeBar,
};
use crate::shared::protocol::GameConfig;

// Sistema para mantener el nombre del jugador siempre horizontal (sin rotar)
pub fn keep_name_horizontal(
    mut name_query: Query<(&mut Transform, &ChildOf), With<PlayerNameText>>,
    parent_query: Query<&Transform, (With<RemotePlayer>, Without<PlayerNameText>)>,
) {
    for (mut name_transform, child_of) in name_query.iter_mut() {
        if let Ok(parent_transform) = parent_query.get(child_of.parent()) {
            // Contrarrestar la rotación del padre para que el texto quede horizontal
            name_transform.rotation = parent_transform.rotation.inverse();
        }
    }
}

pub fn update_charge_bar(
    player_query: Query<(&RemotePlayer, &Children)>,
    config: Res<GameConfig>,
    mut sprite_query: Query<&mut Sprite>,
    mut mesh_query: Query<&mut Mesh2d>,
    // Necesitamos acceso mutable a los materiales para cambiar el color
    mut materials: ResMut<Assets<ColorMaterial>>,
    material_query: Query<&MeshMaterial2d<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    bar_main_q: Query<Entity, With<KickChargeBar>>,
    bar_left_q: Query<Entity, With<KickChargeBarCurveLeft>>,
    bar_right_q: Query<Entity, With<KickChargeBarCurveRight>>,
    player_outline_q: Query<Entity, With<PlayerOutline>>,
) {
    let max_width = 45.0;
    let radius = config.sphere_radius;
    let outline_thickness = 3.0;
    let max_outline_thickness = 7.0;

    for (player, children) in player_query.iter() {
        let charge_pct = player.kick_charge.x; // De 0.0 a 1.0

        for child in children.iter() {
            // --- Lógica de Sprites (Barras) ---
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct, 5.0));
                } else if bar_left_q.contains(child) {
                    let coef = if player.kick_charge.y < 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct * coef, 5.0));
                } else if bar_right_q.contains(child) {
                    let coef = if player.kick_charge.y > 0.0 { 0.5 } else { 0.0 };
                    sprite.custom_size = Some(Vec2::new(max_width * charge_pct * coef, 5.0));
                }
            }
            // --- Lógica del Outline (Mesh + Color) ---
            else if player_outline_q.contains(child) {
                // 1. Actualizar el tamaño del Mesh
                if let Ok(mut mesh_handle) = mesh_query.get_mut(child) {
                    let dynamic_thickness = charge_pct * max_outline_thickness;
                    let new_radius = radius + outline_thickness + dynamic_thickness;
                    *mesh_handle = meshes.add(Circle::new(new_radius)).into();
                }

                // 2. Actualizar el Color (de Negro a Blanco)
                if let Ok(mat_handle) = material_query.get(child) {
                    if let Some(material) = materials.get_mut(mat_handle) {
                        let r = charge_pct; // De 0.0 a 1.0
                        let g = charge_pct;
                        let b = charge_pct;
                        material.color = Color::LinearRgba(LinearRgba::new(r, g, b, 1.0));
                    }
                }
            }
        }
    }
}

pub fn update_dash_cooldown(
    player_query: Query<(&RemotePlayer, &Children)>,
    // Una sola query mutable para el Sprite evita el conflicto B0001
    mut sprite_query: Query<&mut Sprite>,
    // Queries de solo lectura para identificar qué tipo de barra es cada hijo
    bar_main_q: Query<Entity, With<StaminChargeBar>>,
) {
    let max_width = 30.0;

    for (player, children) in player_query.iter() {
        for child in children.iter() {
            // Intentamos obtener el sprite del hijo
            if let Ok(mut sprite) = sprite_query.get_mut(child) {
                // 1. Caso: Barra Principal
                if bar_main_q.contains(child) {
                    sprite.custom_size = Some(Vec2::new(max_width * player.stamin_charge, 5.0));
                }
            }
        }
    }
}

pub fn update_player_sprite(
    player_query: Query<&RemotePlayer>,
    sprite_query: Query<(&PlayerSprite, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (player_sprite, material_handle) in sprite_query.iter() {
        // Buscamos al jugador padre para obtener su color base y estado
        if let Some(player) = player_query
            .iter()
            .find(|p| p.id == player_sprite.parent_id)
        {
            // Aplicar color y transparencia al material
            let alpha = if player.not_interacting { 0.3 } else { 1.0 };

            if let Some(material) = materials.get_mut(&material_handle.0) {
                material.color = player.base_color.with_alpha(alpha);
            }
        }
    }
}

// Sistema para actualizar visualización del cubo según modo activo
pub fn update_mode_visuals(
    player_query: Query<(&RemotePlayer, &Children)>,
    mut cube_query: Query<(&SlideCubeVisual, &mut Transform)>,
    mut sphere_query: Query<(&PlayerSprite, &mut Transform), Without<SlideCubeVisual>>,
    config: Res<GameConfig>,
) {
    for (player, children) in player_query.iter() {
        // Buscar el cubo y la esfera hijos de este jugador
        for child in children.iter() {
            // Actualizar cubo
            if let Ok((cube_visual, mut cube_transform)) = cube_query.get_mut(child) {
                if cube_visual.parent_id != player.id {
                    continue;
                }

                // Si hay un movimiento activo, no sobreescribir (el sistema de movimientos tiene prioridad)
                if player.active_movement.is_some() && player.is_sliding {
                    continue;
                }

                if player.mode_cube_active {
                    // Modo cubo: grande y centrado
                    cube_transform.scale = Vec3::splat(2.5);
                    cube_transform.translation.x = 0.0;
                    cube_transform.translation.y = 0.0;
                } else {
                    // Modo normal: pequeño y en offset
                    cube_transform.scale = Vec3::splat(1.0);
                    cube_transform.translation.x = config.sphere_radius * 0.7;
                    cube_transform.translation.y = 0.0;
                }
            }

            // Actualizar esfera (escala)
            if let Ok((sprite, mut sprite_transform)) = sphere_query.get_mut(child) {
                if sprite.parent_id != player.id {
                    continue;
                }

                if player.mode_cube_active {
                    // Modo cubo: esfera chica
                    sprite_transform.scale = Vec3::splat(0.3);
                } else {
                    // Modo normal: esfera tamaño normal
                    sprite_transform.scale = Vec3::splat(1.0);
                }
            }
        }
    }
}
