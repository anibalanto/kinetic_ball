use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ============================================
// SerializableKeyCode - Wrapper para serde
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerializableKeyCode(pub KeyCode);

impl Serialize for SerializableKeyCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{:?}", self.0))
    }
}

impl<'de> Deserialize<'de> for SerializableKeyCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        key_code_from_str(&s)
            .map(SerializableKeyCode)
            .ok_or_else(|| serde::de::Error::custom(format!("Unknown key: {}", s)))
    }
}

/// Parse string to KeyCode
fn key_code_from_str(s: &str) -> Option<KeyCode> {
    match s {
        // Arrow keys
        "ArrowUp" => Some(KeyCode::ArrowUp),
        "ArrowDown" => Some(KeyCode::ArrowDown),
        "ArrowLeft" => Some(KeyCode::ArrowLeft),
        "ArrowRight" => Some(KeyCode::ArrowRight),

        // Letter keys
        "KeyA" => Some(KeyCode::KeyA),
        "KeyB" => Some(KeyCode::KeyB),
        "KeyC" => Some(KeyCode::KeyC),
        "KeyD" => Some(KeyCode::KeyD),
        "KeyE" => Some(KeyCode::KeyE),
        "KeyF" => Some(KeyCode::KeyF),
        "KeyG" => Some(KeyCode::KeyG),
        "KeyH" => Some(KeyCode::KeyH),
        "KeyI" => Some(KeyCode::KeyI),
        "KeyJ" => Some(KeyCode::KeyJ),
        "KeyK" => Some(KeyCode::KeyK),
        "KeyL" => Some(KeyCode::KeyL),
        "KeyM" => Some(KeyCode::KeyM),
        "KeyN" => Some(KeyCode::KeyN),
        "KeyO" => Some(KeyCode::KeyO),
        "KeyP" => Some(KeyCode::KeyP),
        "KeyQ" => Some(KeyCode::KeyQ),
        "KeyR" => Some(KeyCode::KeyR),
        "KeyS" => Some(KeyCode::KeyS),
        "KeyT" => Some(KeyCode::KeyT),
        "KeyU" => Some(KeyCode::KeyU),
        "KeyV" => Some(KeyCode::KeyV),
        "KeyW" => Some(KeyCode::KeyW),
        "KeyX" => Some(KeyCode::KeyX),
        "KeyY" => Some(KeyCode::KeyY),
        "KeyZ" => Some(KeyCode::KeyZ),

        // Digits
        "Digit0" => Some(KeyCode::Digit0),
        "Digit1" => Some(KeyCode::Digit1),
        "Digit2" => Some(KeyCode::Digit2),
        "Digit3" => Some(KeyCode::Digit3),
        "Digit4" => Some(KeyCode::Digit4),
        "Digit5" => Some(KeyCode::Digit5),
        "Digit6" => Some(KeyCode::Digit6),
        "Digit7" => Some(KeyCode::Digit7),
        "Digit8" => Some(KeyCode::Digit8),
        "Digit9" => Some(KeyCode::Digit9),

        // Modifiers
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),
        "SuperLeft" => Some(KeyCode::SuperLeft),
        "SuperRight" => Some(KeyCode::SuperRight),

        // Special keys
        "Space" => Some(KeyCode::Space),
        "Enter" => Some(KeyCode::Enter),
        "Escape" => Some(KeyCode::Escape),
        "Backspace" => Some(KeyCode::Backspace),
        "Tab" => Some(KeyCode::Tab),
        "CapsLock" => Some(KeyCode::CapsLock),

        // Function keys
        "F1" => Some(KeyCode::F1),
        "F2" => Some(KeyCode::F2),
        "F3" => Some(KeyCode::F3),
        "F4" => Some(KeyCode::F4),
        "F5" => Some(KeyCode::F5),
        "F6" => Some(KeyCode::F6),
        "F7" => Some(KeyCode::F7),
        "F8" => Some(KeyCode::F8),
        "F9" => Some(KeyCode::F9),
        "F10" => Some(KeyCode::F10),
        "F11" => Some(KeyCode::F11),
        "F12" => Some(KeyCode::F12),

        // Punctuation
        "Comma" => Some(KeyCode::Comma),
        "Period" => Some(KeyCode::Period),
        "Slash" => Some(KeyCode::Slash),
        "Semicolon" => Some(KeyCode::Semicolon),
        "Quote" => Some(KeyCode::Quote),
        "BracketLeft" => Some(KeyCode::BracketLeft),
        "BracketRight" => Some(KeyCode::BracketRight),
        "Backslash" => Some(KeyCode::Backslash),
        "Minus" => Some(KeyCode::Minus),
        "Equal" => Some(KeyCode::Equal),
        "Backquote" => Some(KeyCode::Backquote),

        // Numpad
        "Numpad0" => Some(KeyCode::Numpad0),
        "Numpad1" => Some(KeyCode::Numpad1),
        "Numpad2" => Some(KeyCode::Numpad2),
        "Numpad3" => Some(KeyCode::Numpad3),
        "Numpad4" => Some(KeyCode::Numpad4),
        "Numpad5" => Some(KeyCode::Numpad5),
        "Numpad6" => Some(KeyCode::Numpad6),
        "Numpad7" => Some(KeyCode::Numpad7),
        "Numpad8" => Some(KeyCode::Numpad8),
        "Numpad9" => Some(KeyCode::Numpad9),
        "NumpadAdd" => Some(KeyCode::NumpadAdd),
        "NumpadSubtract" => Some(KeyCode::NumpadSubtract),
        "NumpadMultiply" => Some(KeyCode::NumpadMultiply),
        "NumpadDivide" => Some(KeyCode::NumpadDivide),
        "NumpadEnter" => Some(KeyCode::NumpadEnter),
        "NumpadDecimal" => Some(KeyCode::NumpadDecimal),

        // Navigation
        "Insert" => Some(KeyCode::Insert),
        "Delete" => Some(KeyCode::Delete),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "PageUp" => Some(KeyCode::PageUp),
        "PageDown" => Some(KeyCode::PageDown),

        _ => None,
    }
}

/// Get display name for a KeyCode
pub fn key_code_display_name(key: KeyCode) -> String {
    match key {
        KeyCode::ArrowUp => "Up".to_string(),
        KeyCode::ArrowDown => "Down".to_string(),
        KeyCode::ArrowLeft => "Left".to_string(),
        KeyCode::ArrowRight => "Right".to_string(),
        KeyCode::Space => "Space".to_string(),
        KeyCode::ShiftLeft => "L-Shift".to_string(),
        KeyCode::ShiftRight => "R-Shift".to_string(),
        KeyCode::ControlLeft => "L-Ctrl".to_string(),
        KeyCode::ControlRight => "R-Ctrl".to_string(),
        KeyCode::AltLeft => "L-Alt".to_string(),
        KeyCode::AltRight => "R-Alt".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Escape => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        other => format!("{:?}", other).replace("Key", ""),
    }
}

// ============================================
// KeyBindingsConfig - Configuración principal
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct KeyBindingsConfig {
    pub move_up: SerializableKeyCode,
    pub move_down: SerializableKeyCode,
    pub move_left: SerializableKeyCode,
    pub move_right: SerializableKeyCode,
    pub kick: SerializableKeyCode,
    pub curve_left: SerializableKeyCode,
    pub curve_right: SerializableKeyCode,
    pub stop_interact: SerializableKeyCode,
    pub sprint: SerializableKeyCode,
    pub mode: SerializableKeyCode,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        Self {
            move_up: SerializableKeyCode(KeyCode::ArrowUp),
            move_down: SerializableKeyCode(KeyCode::ArrowDown),
            move_left: SerializableKeyCode(KeyCode::ArrowLeft),
            move_right: SerializableKeyCode(KeyCode::ArrowRight),
            kick: SerializableKeyCode(KeyCode::KeyS),
            curve_left: SerializableKeyCode(KeyCode::KeyD),
            curve_right: SerializableKeyCode(KeyCode::KeyA),
            stop_interact: SerializableKeyCode(KeyCode::ShiftLeft),
            sprint: SerializableKeyCode(KeyCode::Space),
            mode: SerializableKeyCode(KeyCode::ControlLeft),
        }
    }
}

// ============================================
// GameAction - Enum para UI
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Kick,
    CurveLeft,
    CurveRight,
    StopInteract,
    Sprint,
    Mode,
}

impl GameAction {
    pub fn all() -> &'static [GameAction] {
        &[
            GameAction::MoveUp,
            GameAction::MoveDown,
            GameAction::MoveLeft,
            GameAction::MoveRight,
            GameAction::Kick,
            GameAction::CurveLeft,
            GameAction::CurveRight,
            GameAction::StopInteract,
            GameAction::Sprint,
            GameAction::Mode,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            GameAction::MoveUp => "Mover Arriba",
            GameAction::MoveDown => "Mover Abajo",
            GameAction::MoveLeft => "Mover Izquierda",
            GameAction::MoveRight => "Mover Derecha",
            GameAction::Kick => "Patear",
            GameAction::CurveLeft => "Curva Izquierda",
            GameAction::CurveRight => "Curva Derecha",
            GameAction::StopInteract => "Frenar",
            GameAction::Sprint => "Sprint",
            GameAction::Mode => "Modo",
        }
    }
}

impl KeyBindingsConfig {
    pub fn get_key(&self, action: GameAction) -> KeyCode {
        match action {
            GameAction::MoveUp => self.move_up.0,
            GameAction::MoveDown => self.move_down.0,
            GameAction::MoveLeft => self.move_left.0,
            GameAction::MoveRight => self.move_right.0,
            GameAction::Kick => self.kick.0,
            GameAction::CurveLeft => self.curve_left.0,
            GameAction::CurveRight => self.curve_right.0,
            GameAction::StopInteract => self.stop_interact.0,
            GameAction::Sprint => self.sprint.0,
            GameAction::Mode => self.mode.0,
        }
    }

    pub fn set_key(&mut self, action: GameAction, key: KeyCode) {
        let key = SerializableKeyCode(key);
        match action {
            GameAction::MoveUp => self.move_up = key,
            GameAction::MoveDown => self.move_down = key,
            GameAction::MoveLeft => self.move_left = key,
            GameAction::MoveRight => self.move_right = key,
            GameAction::Kick => self.kick = key,
            GameAction::CurveLeft => self.curve_left = key,
            GameAction::CurveRight => self.curve_right = key,
            GameAction::StopInteract => self.stop_interact = key,
            GameAction::Sprint => self.sprint = key,
            GameAction::Mode => self.mode = key,
        }
    }
}

// ============================================
// SettingsUIState - Estado de la UI
// ============================================

#[derive(Resource, Default)]
pub struct SettingsUIState {
    pub rebinding_action: Option<GameAction>,
    pub pending_bindings: Option<KeyBindingsConfig>,
    pub status_message: Option<String>,
}

impl SettingsUIState {
    pub fn start_rebind(&mut self, action: GameAction) {
        self.rebinding_action = Some(action);
        self.status_message = Some(format!(
            "Presiona una tecla para '{}'... (ESC para cancelar)",
            action.display_name()
        ));
    }

    pub fn cancel_rebind(&mut self) {
        self.rebinding_action = None;
        self.status_message = None;
    }

    pub fn is_rebinding(&self) -> bool {
        self.rebinding_action.is_some()
    }
}

// ============================================
// Carga y guardado de archivo
// ============================================

pub fn get_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("rustball"))
}

pub fn get_keybindings_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join("keybindings.ron"))
}

pub fn load_keybindings() -> KeyBindingsConfig {
    let Some(path) = get_keybindings_path() else {
        println!("[Config] No se pudo determinar la ruta de config, usando defaults");
        return KeyBindingsConfig::default();
    };

    match fs::read_to_string(&path) {
        Ok(content) => match ron::from_str::<KeyBindingsConfig>(&content) {
            Ok(config) => {
                println!("[Config] Keybindings cargados desde {:?}", path);
                config
            }
            Err(e) => {
                println!(
                    "[Config] Error parseando keybindings ({}), usando defaults",
                    e
                );
                KeyBindingsConfig::default()
            }
        },
        Err(_) => {
            println!("[Config] No existe archivo de keybindings, usando defaults");
            KeyBindingsConfig::default()
        }
    }
}

pub fn save_keybindings(config: &KeyBindingsConfig) -> Result<(), String> {
    let config_dir = get_config_dir().ok_or("No se pudo determinar directorio de config")?;

    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Error creando directorio de config: {}", e))?;

    let path = config_dir.join("keybindings.ron");

    let content = ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("Error serializando config: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Error escribiendo archivo: {}", e))?;

    println!("[Config] Keybindings guardados en {:?}", path);
    Ok(())
}
