/// Módulo de abstracción de input para el juego.
/// Permite recibir input de keyboard, joystick o red sin dependencias directas.

/// Acciones del juego que pueden ser mapeadas a diferentes fuentes de input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Kick,
    CurveLeft,
    CurveRight,
    StopInteract,  // Equivalente a Space/soltar la pelota
    Sprint,        // Equivalente a Shift
}

/// Estado de un botón o acción
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    Pressed,
    Released,
}

/// Trait que debe implementar cualquier fuente de input
pub trait InputSource: Send + Sync {
    /// Retorna true si la acción está siendo presionada en este frame
    fn is_pressed(&self, action: GameAction) -> bool;

    /// Retorna true si la acción fue presionada en este frame (just_pressed)
    fn just_pressed(&self, action: GameAction) -> bool;

    /// Retorna true si la acción fue soltada en este frame (just_released)
    fn just_released(&self, action: GameAction) -> bool;

    /// Actualiza el estado interno (llamar una vez por frame)
    fn update(&mut self);
}

/// Agregador de múltiples fuentes de input
/// Permite combinar keyboard + joystick + red
pub struct InputManager {
    sources: Vec<Box<dyn InputSource>>,
}

impl InputManager {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn add_source(&mut self, source: Box<dyn InputSource>) {
        self.sources.push(source);
    }

    /// Retorna true si ALGUNA fuente tiene la acción presionada
    pub fn is_pressed(&self, action: GameAction) -> bool {
        self.sources.iter().any(|s| s.is_pressed(action))
    }

    /// Retorna true si ALGUNA fuente tiene just_pressed
    pub fn just_pressed(&self, action: GameAction) -> bool {
        self.sources.iter().any(|s| s.just_pressed(action))
    }

    /// Retorna true si ALGUNA fuente tiene just_released
    pub fn just_released(&self, action: GameAction) -> bool {
        self.sources.iter().any(|s| s.just_released(action))
    }

    /// Actualiza todas las fuentes
    pub fn update(&mut self) {
        for source in &mut self.sources {
            source.update();
        }
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
