use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use shared::map::{Map, Vertex, Segment, CurveConfig};

pub struct MapConverter {
    curve_config: CurveConfig,
}

impl MapConverter {
    pub fn new() -> Self {
        Self {
            curve_config: CurveConfig::default(),
        }
    }

    pub fn with_curve_config(curve_config: CurveConfig) -> Self {
        Self { curve_config }
    }

    /// Spawnear toda la geometría del mapa en el mundo ECS
    pub fn spawn_map_geometry(
        &self,
        commands: &mut Commands,
        map: &Map,
        default_restitution: f32,
    ) {
        println!("🗺️  Spawning map geometry: {}", map.name);

        // Spawnear segmentos (paredes)
        for (i, segment) in map.segments.iter().enumerate() {
            if let Some(collider) = self.segment_to_collider(segment, &map.vertexes) {
                let collision_groups = self.compute_collision_groups(
                    segment.c_mask.as_ref(),
                    segment.c_group.as_ref(),
                );

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
                println!("  ✓ Segment {}: v{}({:.0},{:.0}) → v{}({:.0},{:.0})",
                    i, segment.v0, v0.x, v0.y, segment.v1, v1.x, v1.y);
            }
        }

        // Spawnear discos
        for (i, disc) in map.discs.iter().enumerate() {
            let collision_groups = self.compute_collision_groups(
                disc.c_mask.as_ref(),
                disc.c_group.as_ref(),
            );

            commands.spawn((
                RigidBody::Fixed,
                Collider::ball(disc.radius),
                collision_groups,
                Restitution::coefficient(disc.b_coef),
                Transform::from_xyz(disc.pos[0], disc.pos[1], 0.0),
                GlobalTransform::default(),
            ));

            println!("  ✓ Disc {}: pos=({:.0}, {:.0}), r={:.0}",
                i, disc.pos[0], disc.pos[1], disc.radius);
        }

        println!("✅ Spawned {} segments, {} discs",
            map.segments.len(), map.discs.len());
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
            let rapier_points: Vec<_> = points
                .into_iter()
                .map(|p| [p.x, p.y].into())
                .collect();

            Collider::polyline(rapier_points, None)
        }
    }

    /// Aproximar segmento curvo con polyline
    fn approximate_curve(&self, p0: Vec2, p1: Vec2, curve: f32) -> Vec<Vec2> {
        let num_segments = self.curve_config.segments_per_curve;
        let mut points = Vec::with_capacity(num_segments + 1);

        // En HaxBall:
        // - curve > 0: curva hacia la derecha
        // - curve < 0: curva hacia la izquierda
        // - |curve| = radio de curvatura del arco

        let dir = (p1 - p0).normalize();
        let length = p0.distance(p1);

        // Vector perpendicular (apunta hacia donde se curva)
        let perp = Vec2::new(-dir.y, dir.x);

        // Radio del arco
        let radius = curve.abs();

        // Ángulo del arco
        let arc_angle = length / radius;

        // Centro del círculo
        let center = (p0 + p1) * 0.5 + perp * curve;

        // Generar puntos a lo largo del arco
        for i in 0..=num_segments {
            let t = i as f32 / num_segments as f32;
            let angle = -arc_angle * 0.5 + arc_angle * t;

            let offset_angle = if curve > 0.0 {
                dir.y.atan2(dir.x) + std::f32::consts::FRAC_PI_2
            } else {
                dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2
            };

            let point = center + Vec2::new(
                radius * (offset_angle + angle).cos(),
                radius * (offset_angle + angle).sin(),
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
        // Por defecto: comportamiento tipo pared (GROUP_1, colisiona con todos)
        let mut memberships = Group::GROUP_1;
        let mut filters = Group::ALL;

        // Parsear grupos de colisión de HaxBall
        if let Some(groups) = cgroup {
            memberships = self.parse_group_membership(groups);
        }

        if let Some(masks) = cmask {
            filters = self.parse_group_filters(masks);
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

        if result == Group::NONE {
            result = Group::ALL; // Por defecto: colisiona con todo
        }

        result
    }
}

impl Default for MapConverter {
    fn default() -> Self {
        Self::new()
    }
}
