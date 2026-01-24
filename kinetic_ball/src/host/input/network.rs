/// NetworkInputSource - Implementación de InputSource para jugadores conectados por red
/// Almacena el input actual y anterior para detectar just_pressed/just_released
use super::core::{GameAction, InputSource};
use crate::shared::protocol::PlayerInput;

pub struct NetworkInputSource {
    current: PlayerInput,
    previous: PlayerInput,
}

impl NetworkInputSource {
    pub fn new() -> Self {
        Self {
            current: PlayerInput::default(),
            previous: PlayerInput::default(),
        }
    }

    /// Actualiza el input con uno nuevo recibido de la red
    pub fn set_input(&mut self, input: PlayerInput) {
        self.current = input;
    }
}

impl InputSource for NetworkInputSource {
    fn is_pressed(&self, action: GameAction) -> bool {
        match action {
            GameAction::MoveUp => self.current.move_up,
            GameAction::MoveDown => self.current.move_down,
            GameAction::MoveLeft => self.current.move_left,
            GameAction::MoveRight => self.current.move_right,
            GameAction::Kick => self.current.kick,
            GameAction::CurveLeft => self.current.curve_left,
            GameAction::CurveRight => self.current.curve_right,
            GameAction::StopInteract => self.previous.stop_interact,
            GameAction::Dash => self.current.dash,
            GameAction::Sprint => self.current.sprint,
            GameAction::Mode => self.current.mode,
        }
    }

    fn just_pressed(&self, action: GameAction) -> bool {
        let current = self.is_pressed(action);
        let previous = match action {
            GameAction::MoveUp => self.previous.move_up,
            GameAction::MoveDown => self.previous.move_down,
            GameAction::MoveLeft => self.previous.move_left,
            GameAction::MoveRight => self.previous.move_right,
            GameAction::Kick => self.previous.kick,
            GameAction::CurveLeft => self.previous.curve_left,
            GameAction::CurveRight => self.previous.curve_right,
            GameAction::StopInteract => self.previous.stop_interact,
            GameAction::Dash => self.previous.dash,
            GameAction::Sprint => self.previous.sprint,
            GameAction::Mode => self.previous.mode,
        };
        current && !previous
    }

    fn just_released(&self, action: GameAction) -> bool {
        let current = self.is_pressed(action);
        let previous = match action {
            GameAction::MoveUp => self.previous.move_up,
            GameAction::MoveDown => self.previous.move_down,
            GameAction::MoveLeft => self.previous.move_left,
            GameAction::MoveRight => self.previous.move_right,
            GameAction::Kick => self.previous.kick,
            GameAction::CurveLeft => self.previous.curve_left,
            GameAction::CurveRight => self.previous.curve_right,
            GameAction::StopInteract => self.previous.stop_interact,
            GameAction::Dash => self.previous.dash,
            GameAction::Sprint => self.previous.sprint,
            GameAction::Mode => self.previous.mode,
        };
        !current && previous
    }

    fn update(&mut self) {
        self.previous = self.current.clone();
    }
}
