use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use crate::shared::map::{CurveConfig, Map, Segment, Vertex};

pub struct MapConverter {
    curve_config: CurveConfig,
}

impl MapConverter {
    pub fn new() -> Self {
        Self {
            curve_config: CurveConfig::default(),
        }
    }

    /// Spawnear toda la geometría del mapa en el mundo ECS
    pub fn spawn_map_geometry(&self, commands: &mut Commands, map: &Map) {
        println!("🗺️  Spawning map geometry: {}", map.name);

        // Spawnear segmentos (paredes)
        for (i, segment) in map.segments.iter().enumerate() {
            // Skip segmentos invisibles - no tienen física, solo son decorativos
            if !segment.is_visible() {
                println!("  ⊘ Segment {} (invisible): skipping physics", i);
                continue;
            }

            if let Some(collider) = self.segment_to_collider(segment, &map.vertexes) {
                let collision_groups = self
                    .compute_collision_groups(segment.c_mask.as_ref(), segment.c_group.as_ref());

                let restitution = segment.b_coef;

                commands.spawn((
                    RigidBody::Fixed,
                    collider,
                    collision_groups,
                    Restitution::coefficient(restitution),
                    Transform::default(),
                    GlobalTransform::default(),
                ));

                let v0 = &map.vertexes[segment.v0];
                let v1 = &map.vertexes[segment.v1];
                println!(
                    "  ✓ Segment {}: v{}({:.0},{:.0}) → v{}({:.0},{:.0})",
                    i, segment.v0, v0.x, v0.y, segment.v1, v1.x, v1.y
                );
            }
        }

        // Spawnear discos
        for (i, disc) in map.discs.iter().enumerate() {
            let collision_groups =
                self.compute_collision_groups(disc.c_mask.as_ref(), disc.c_group.as_ref());

            commands.spawn((
                RigidBody::Fixed,
                Collider::ball(disc.radius),
                collision_groups,
                Restitution::coefficient(disc.b_coef),
                Transform::from_xyz(disc.pos[0], disc.pos[1], 0.0),
                GlobalTransform::default(),
            ));

            println!(
                "  ✓ Disc {}: pos=({:.0}, {:.0}), r={:.0}",
                i, disc.pos[0], disc.pos[1], disc.radius
            );
        }

        println!(
            "✅ Spawned {} segments, {} discs",
            map.segments.len(),
            map.discs.len()
        );
    }

    /// Convertir segmento a collider (maneja rectos y curvos)
    fn segment_to_collider(&self, segment: &Segment, vertices: &[Vertex]) -> Option<Collider> {
        let v0 = &vertices[segment.v0];
        let v1 = &vertices[segment.v1];

        let p0 = Vec2::new(v0.x, v0.y);
        let p1 = Vec2::new(v1.x, v1.y);

        // Verificar si el segmento es curvo
        let curve_factor = segment.curve.or(segment.curve_f).unwrap_or(0.0);

        if curve_factor.abs() < 0.01 {
            // Segmento recto
            Some(Collider::segment(p0.into(), p1.into()))
        } else {
            // Segmento curvo - aproximar con polyline
            let points = self.approximate_curve(p0, p1, curve_factor);

            // Convertir Vec<Vec2> a Vec<Point>
            let rapier_points: Vec<_> = points.into_iter().map(|p| [p.x, p.y].into()).collect();

            Some(Collider::polyline(rapier_points, None))
        }
    }

    /// Aproximar segmento curvo con polyline (HaxBall curve format)
    fn approximate_curve(&self, p0: Vec2, p1: Vec2, curve: f32) -> Vec<Vec2> {
        let num_segments = self.curve_config.segments_per_curve;
        let mut points = Vec::with_capacity(num_segments + 1);

        let chord = p0.distance(p1);
        let radius = curve.abs();

        // Si el radio es muy pequeño o inválido, retornar línea recta
        if radius < chord / 2.0 {
            points.push(p0);
            points.push(p1);
            return points;
        }

        // Calcular el ángulo subtendido por la cuerda
        let half_angle = (chord / (2.0 * radius)).asin();
        let total_angle = 2.0 * half_angle;

        // Punto medio de la cuerda
        let midpoint = (p0 + p1) * 0.5;

        // Vector de p0 a p1
        let chord_vec = p1 - p0;

        // Vector perpendicular (normalizado)
        let perp = Vec2::new(-chord_vec.y, chord_vec.x).normalize();

        // Distancia del centro a la cuerda
        let height = (radius * radius - (chord / 2.0) * (chord / 2.0)).sqrt();

        // Centro del círculo (curva positiva = perp positivo, negativa = perp negativo)
        let center = if curve > 0.0 {
            midpoint + perp * height
        } else {
            midpoint - perp * height
        };

        // Ángulo inicial (de center a p0)
        let start_angle = (p0.y - center.y).atan2(p0.x - center.x);

        // Determinar dirección de barrido
        let angle_step = if curve > 0.0 {
            -total_angle / num_segments as f32
        } else {
            total_angle / num_segments as f32
        };

        // Generar puntos
        for i in 0..=num_segments {
            let angle = start_angle + angle_step * i as f32;
            let point = Vec2::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            );
            points.push(point);
        }

        points
    }

    /// Mapear máscara/grupo de colisión de HaxBall a grupos de Rapier2D
    fn compute_collision_groups(
        &self,
        cmask: Option<&Vec<String>>,
        cgroup: Option<&Vec<String>>,
    ) -> CollisionGroups {
        // Por defecto: sin colisión (líneas decorativas)
        let mut memberships = Group::GROUP_1;
        let mut filters = Group::NONE;

        // Parsear cMask primero para determinar memberships especiales
        if let Some(masks) = cmask {
            filters = self.parse_group_filters(masks);

            // Usar memberships especiales según cMask
            if masks.iter().any(|m| m == "ball") && !masks.iter().any(|m| m == "red" || m == "blue")
            {
                // Solo pelota: usar GROUP_5
                memberships = Group::GROUP_5;
            } else if masks.iter().any(|m| m == "red" || m == "blue")
                && !masks.iter().any(|m| m == "ball")
            {
                // Solo jugadores: usar GROUP_6
                memberships = Group::GROUP_6;
            }
        }

        // Si tiene cGroup explícito, usarlo (override)
        if let Some(groups) = cgroup {
            let parsed = self.parse_group_membership(groups);
            if parsed != Group::NONE {
                memberships = parsed;
            }
        }

        CollisionGroups::new(memberships, filters)
    }

    fn parse_group_membership(&self, groups: &[String]) -> Group {
        let mut result = Group::NONE;

        for group in groups {
            match group.as_str() {
                "wall" => result |= Group::GROUP_1,
                "kick" => result |= Group::GROUP_2,
                "ball" => result |= Group::GROUP_3,
                "red" | "blue" => result |= Group::GROUP_4,
                _ => {} // Grupo desconocido, ignorar
            }
        }

        if result == Group::NONE {
            result = Group::GROUP_1; // Por defecto: pared
        }

        result
    }

    fn parse_group_filters(&self, masks: &[String]) -> Group {
        let mut result = Group::NONE;

        for mask in masks {
            match mask.as_str() {
                "wall" => result |= Group::GROUP_1,
                "kick" => result |= Group::GROUP_2,
                "ball" => result |= Group::GROUP_3,
                "red" | "blue" | "c0" | "c1" => result |= Group::GROUP_4,
                "all" => result = Group::ALL,
                _ => {}
            }
        }

        // Si cMask está vacío o no definido, NO colisionar con nada
        // Solo las líneas con cMask explícito deberían tener física
        result
    }
}

impl Default for MapConverter {
    fn default() -> Self {
        Self::new()
    }
}
