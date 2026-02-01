use bevy::prelude::*;

use crate::shared::protocol::PlayerMovement;

// ============================================================================
// GAME LIFECYCLE
// ============================================================================

/// Marcador para todas las entidades creadas durante InGame.
/// Se usa para limpiar todo al salir de la sala.
#[derive(Component)]
pub struct InGameEntity;

// ============================================================================
// FIELD COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct DefaultFieldLine;

#[derive(Component)]
pub struct MinimapFieldLine;

#[derive(Component)]
pub struct MapLineEntity; // Líneas del mapa cargado (reemplazo de Gizmos)

#[derive(Component)]
pub struct FieldBackground;

#[derive(Component)]
pub struct MinimapFieldBackground;

// ============================================================================
// CAMERA COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct MenuCamera;

#[derive(Component)]
pub struct MinimapCamera;

#[derive(Component)]
pub struct PlayerDetailCamera {
    pub local_index: u8,
}

/// Cámara dedicada para UI que no tiene viewport (renderiza pantalla completa)
#[derive(Component)]
pub struct GameUiCamera;

/// Cámara que compone el split-screen final
#[derive(Component)]
pub struct CompositorCamera;

#[derive(Component)]
pub struct PlayerCamera {
    pub local_index: u8,
    pub server_player_id: Option<u32>,
}

// ============================================================================
// SPLIT SCREEN COMPONENTS
// ============================================================================

/// Componente para identificar el mesh que muestra el split-screen compuesto
#[derive(Component)]
pub struct SplitScreenQuad;

// ============================================================================
// PLAYER & BALL COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct RemotePlayer {
    pub id: u32,
    pub name: String,
    pub team_index: u8,
    pub kick_charge: Vec2, // x = potencia, y = curva
    pub is_sliding: bool,
    pub not_interacting: bool,
    pub base_color: Color,
    pub ball_target_position: Option<Vec2>,
    pub stamin_charge: f32,
    pub active_movement: Option<PlayerMovement>,
    pub mode_cube_active: bool,
}

#[derive(Component)]
pub struct RemoteBall;

#[derive(Component)]
pub struct Interpolated {
    pub target_position: Vec2,
    pub target_velocity: Vec2,
    pub target_rotation: f32,
    pub smoothing: f32,
}

// ============================================================================
// PLAYER UI COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct KickChargeBar;

#[derive(Component)]
pub struct KickChargeBarCurveLeft;

#[derive(Component)]
pub struct KickChargeBarCurveRight;

#[derive(Component)]
pub struct StaminChargeBar;

#[derive(Component)]
pub struct PlayerNameText;

#[derive(Component)]
pub struct PlayerSprite {
    pub parent_id: u32,
}

#[derive(Component)]
pub struct PlayerOutline;

#[derive(Component)]
pub struct SlideCubeVisual {
    pub parent_id: u32,
}

// ============================================================================
// MINIMAP COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct MinimapDot {
    pub tracks_entity: Entity,
}

#[derive(Component)]
pub struct MinimapPlayerName {
    pub tracks_entity: Entity,
}

// ============================================================================
// KEY VISUAL COMPONENTS
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CurveAction {
    Left,
    Right,
}

#[derive(Component)]
pub struct KeyVisual {
    pub player_id: u32,
    pub action: CurveAction,
}
