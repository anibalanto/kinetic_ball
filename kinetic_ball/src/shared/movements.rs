use serde::{Deserialize, Serialize};

// ============================================================================
// SISTEMA DE MOVIMIENTOS CON KEYFRAMES
// ============================================================================

/// Objeto objetivo del movimiento
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MovementTarget {
    /// Cubo indicador de dirección del jugador
    DirectionCube,
}

/// Función de easing para la interpolación
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum EasingFunction {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl EasingFunction {
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            EasingFunction::Linear => t,
            EasingFunction::EaseIn => t * t,
            EasingFunction::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            EasingFunction::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
        }
    }
}

/// Un keyframe define un valor en un momento específico del tiempo
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Keyframe {
    /// Tiempo normalizado (0.0 = inicio, 1.0 = fin)
    pub time: f32,
    /// Valor en este keyframe
    pub value: f32,
    /// Easing para interpolar HACIA este keyframe
    pub easing: EasingFunction,
}

impl Keyframe {
    pub fn new(time: f32, value: f32) -> Self {
        Self {
            time,
            value,
            easing: EasingFunction::Linear,
        }
    }

    pub fn with_easing(time: f32, value: f32, easing: EasingFunction) -> Self {
        Self { time, value, easing }
    }
}

/// Track de animación para una propiedad específica
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationTrack {
    pub property: AnimatedProperty,
    pub keyframes: Vec<Keyframe>,
}

impl AnimationTrack {
    /// Evalúa el valor de la propiedad en el tiempo dado (0.0 - 1.0)
    pub fn evaluate(&self, progress: f32) -> f32 {
        if self.keyframes.is_empty() {
            return 0.0;
        }
        if self.keyframes.len() == 1 {
            return self.keyframes[0].value;
        }

        // Encontrar los dos keyframes entre los que estamos
        let mut prev_kf = &self.keyframes[0];
        let mut next_kf = &self.keyframes[0];

        for kf in &self.keyframes {
            if kf.time <= progress {
                prev_kf = kf;
            }
            if kf.time >= progress {
                next_kf = kf;
                break;
            }
        }

        // Si estamos antes del primer keyframe o después del último
        if progress <= prev_kf.time {
            return prev_kf.value;
        }
        if progress >= next_kf.time {
            return next_kf.value;
        }

        // Interpolar entre los dos keyframes
        let segment_duration = next_kf.time - prev_kf.time;
        let segment_progress = (progress - prev_kf.time) / segment_duration;
        let eased_progress = next_kf.easing.apply(segment_progress);

        prev_kf.value + (next_kf.value - prev_kf.value) * eased_progress
    }
}

/// Propiedades que se pueden animar
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AnimatedProperty {
    /// Escala uniforme
    Scale,
    /// Offset en X relativo al jugador (en dirección forward)
    OffsetX,
    /// Offset en Y relativo al jugador (perpendicular)
    OffsetY,
    /// Rotación adicional (radianes, se suma a la base de 45°)
    Rotation,
}

/// Definición de un movimiento con múltiples tracks de animación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Movement {
    pub target: MovementTarget,
    pub tracks: Vec<AnimationTrack>,
    pub duration: f32, // segundos
}

impl Movement {
    /// Crea un nuevo movimiento vacío
    pub fn new(target: MovementTarget, duration: f32) -> Self {
        Self {
            target,
            tracks: Vec::new(),
            duration,
        }
    }

    /// Agrega un track de animación
    pub fn with_track(mut self, property: AnimatedProperty, keyframes: Vec<Keyframe>) -> Self {
        self.tracks.push(AnimationTrack { property, keyframes });
        self
    }

    /// Evalúa una propiedad específica en el tiempo dado
    pub fn evaluate(&self, property: AnimatedProperty, progress: f32) -> Option<f32> {
        self.tracks
            .iter()
            .find(|t| t.property == property)
            .map(|t| t.evaluate(progress))
    }
}

// ============================================================================
// COMPATIBILIDAD CON API ANTERIOR (para transición gradual)
// ============================================================================

/// Propiedad física a animar (API simple, compatible con código anterior)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MovementProperty {
    Scale { from: f32, to: f32 },
    PositionOffset { x: f32, y: f32 },
    Rotation { angle: f32 },
}

// ============================================================================
// REGISTRO DE MOVIMIENTOS PREDEFINIDOS
// ============================================================================

/// IDs de movimientos predefinidos
pub mod movement_ids {
    pub const SLIDE_CUBE_GROW: u8 = 0;
    pub const SLIDE_CUBE_SHRINK: u8 = 1;
    pub const DIRECTION_PULSE: u8 = 2;
}

/// Obtiene un movimiento predefinido por su ID
pub fn get_movement(id: u8) -> Option<Movement> {
    use EasingFunction::*;

    match id {
        movement_ids::SLIDE_CUBE_GROW => Some(
            // El cubo avanza, se agranda y rota 45° adicionales
            Movement::new(MovementTarget::DirectionCube, 0.15)
                // Escala: 1.0 -> 3.0 con ease out
                .with_track(AnimatedProperty::Scale, vec![
                    Keyframe::new(0.0, 1.0),
                    Keyframe::with_easing(1.0, 3.0, EaseOut),
                ])
                // Avanza hacia adelante: base (0.7 * radius) -> 1.8 * radius
                // El valor es multiplicador del radio del jugador
                .with_track(AnimatedProperty::OffsetX, vec![
                    Keyframe::new(0.0, 0.7),
                    Keyframe::with_easing(1.0, 1.8, EaseOut),
                ])
                // Rotación adicional: 0 -> 45° (PI/4)
                .with_track(AnimatedProperty::Rotation, vec![
                    Keyframe::new(0.0, 0.0),
                    Keyframe::with_easing(1.0, std::f32::consts::FRAC_PI_4, EaseOut),
                ])
        ),

        movement_ids::SLIDE_CUBE_SHRINK => Some(
            // El cubo retrocede, se achica y vuelve a su rotación original
            Movement::new(MovementTarget::DirectionCube, 0.2)
                // Escala: 3.0 -> 1.0 con ease in
                .with_track(AnimatedProperty::Scale, vec![
                    Keyframe::new(0.0, 3.0),
                    Keyframe::with_easing(1.0, 1.0, EaseIn),
                ])
                // Retrocede: 1.8 -> 0.7
                .with_track(AnimatedProperty::OffsetX, vec![
                    Keyframe::new(0.0, 1.8),
                    Keyframe::with_easing(1.0, 0.7, EaseIn),
                ])
                // Rotación: 45° -> 0
                .with_track(AnimatedProperty::Rotation, vec![
                    Keyframe::new(0.0, std::f32::consts::FRAC_PI_4),
                    Keyframe::with_easing(1.0, 0.0, EaseIn),
                ])
        ),

        movement_ids::DIRECTION_PULSE => Some(
            // Un pulso rápido: crece y vuelve
            Movement::new(MovementTarget::DirectionCube, 0.2)
                .with_track(AnimatedProperty::Scale, vec![
                    Keyframe::new(0.0, 1.0),
                    Keyframe::with_easing(0.5, 1.5, EaseOut),  // Crece hasta la mitad
                    Keyframe::with_easing(1.0, 1.0, EaseIn),   // Vuelve al final
                ])
        ),

        _ => None,
    }
}

/// Estado de un movimiento en ejecución (para uso local, no requerido con sistema de ticks)
#[derive(Debug, Clone)]
pub struct ActiveMovement {
    pub movement: Movement,
    pub elapsed: f32,
    pub player_id: u32,
}

impl ActiveMovement {
    pub fn new(movement: Movement, player_id: u32) -> Self {
        Self {
            movement,
            elapsed: 0.0,
            player_id,
        }
    }

    /// Avanza el tiempo y retorna el progreso normalizado (0.0 - 1.0)
    /// El easing se aplica por cada track en Movement::evaluate()
    pub fn tick(&mut self, delta: f32) -> f32 {
        self.elapsed += delta;
        (self.elapsed / self.movement.duration).min(1.0)
    }

    /// Retorna true si el movimiento ha terminado
    pub fn is_finished(&self) -> bool {
        self.elapsed >= self.movement.duration
    }

    /// Interpola un valor según el progreso
    pub fn lerp(from: f32, to: f32, progress: f32) -> f32 {
        from + (to - from) * progress
    }
}
