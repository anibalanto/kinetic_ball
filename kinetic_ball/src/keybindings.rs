use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    pub wildcard: SerializableKeyCode, // StopInteract en modo normal, Dash en modo cubo
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
            wildcard: SerializableKeyCode(KeyCode::ShiftLeft),
            sprint: SerializableKeyCode(KeyCode::Space),
            mode: SerializableKeyCode(KeyCode::ControlRight),
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
    Wildcard, // StopInteract en modo normal, Dash en modo cubo
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
            GameAction::Wildcard,
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
            GameAction::Wildcard => "Especial",
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
            GameAction::Wildcard => self.wildcard.0,
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
            GameAction::Wildcard => self.wildcard = key,
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
    dirs::config_dir().map(|p| p.join("kinetic_ball"))
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

// ============================================
// Configuración General (servidor, etc.)
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: "kinetic-ball.fly.dev".to_string(),
        }
    }
}

pub fn get_app_config_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join("config.ron"))
}

pub fn load_app_config() -> AppConfig {
    let Some(path) = get_app_config_path() else {
        println!("[Config] No se pudo determinar la ruta de config, usando defaults");
        return AppConfig::default();
    };

    match fs::read_to_string(&path) {
        Ok(content) => match ron::from_str::<AppConfig>(&content) {
            Ok(config) => {
                println!("[Config] App config cargado desde {:?}", path);
                config
            }
            Err(e) => {
                println!("[Config] Error parseando config: {}, usando defaults", e);
                AppConfig::default()
            }
        },
        Err(_) => {
            println!("[Config] No existe archivo de config, usando defaults");
            AppConfig::default()
        }
    }
}

// ============================================
// Gamepad Bindings - Para gamepads genéricos
// ============================================

/// Tipo de input para un binding de gamepad
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawGamepadInput {
    /// Botón por número (0, 1, 2, etc.)
    Button(u8),
    /// Eje positivo (valor > threshold)
    AxisPositive(u8),
    /// Eje negativo (valor < -threshold)
    AxisNegative(u8),
}

impl RawGamepadInput {
    pub fn display_name(&self) -> String {
        match self {
            RawGamepadInput::Button(n) => format!("Botón {}", n),
            RawGamepadInput::AxisPositive(n) => format!("Eje {}+", n),
            RawGamepadInput::AxisNegative(n) => format!("Eje {}-", n),
        }
    }
}

/// Configuración de bindings para un gamepad genérico
#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct GamepadBindingsConfig {
    pub move_up: Option<RawGamepadInput>,
    pub move_down: Option<RawGamepadInput>,
    pub move_left: Option<RawGamepadInput>,
    pub move_right: Option<RawGamepadInput>,
    pub kick: Option<RawGamepadInput>,
    pub curve_left: Option<RawGamepadInput>,
    pub curve_right: Option<RawGamepadInput>,
    pub wildcard: Option<RawGamepadInput>,
    pub sprint: Option<RawGamepadInput>,
    pub mode: Option<RawGamepadInput>,
}

impl Default for GamepadBindingsConfig {
    fn default() -> Self {
        Self {
            // Por defecto, usar ejes 0 y 1 para movimiento
            move_up: Some(RawGamepadInput::AxisNegative(1)),
            move_down: Some(RawGamepadInput::AxisPositive(1)),
            move_left: Some(RawGamepadInput::AxisNegative(0)),
            move_right: Some(RawGamepadInput::AxisPositive(0)),
            // Botones típicos
            kick: Some(RawGamepadInput::Button(0)),
            curve_left: Some(RawGamepadInput::Button(2)),
            curve_right: Some(RawGamepadInput::Button(1)),
            wildcard: Some(RawGamepadInput::Button(4)),
            sprint: Some(RawGamepadInput::Button(5)),
            mode: Some(RawGamepadInput::Button(3)),
        }
    }
}

impl GamepadBindingsConfig {
    pub fn get_binding(&self, action: GameAction) -> Option<RawGamepadInput> {
        match action {
            GameAction::MoveUp => self.move_up,
            GameAction::MoveDown => self.move_down,
            GameAction::MoveLeft => self.move_left,
            GameAction::MoveRight => self.move_right,
            GameAction::Kick => self.kick,
            GameAction::CurveLeft => self.curve_left,
            GameAction::CurveRight => self.curve_right,
            GameAction::Wildcard => self.wildcard,
            GameAction::Sprint => self.sprint,
            GameAction::Mode => self.mode,
        }
    }

    pub fn set_binding(&mut self, action: GameAction, input: Option<RawGamepadInput>) {
        match action {
            GameAction::MoveUp => self.move_up = input,
            GameAction::MoveDown => self.move_down = input,
            GameAction::MoveLeft => self.move_left = input,
            GameAction::MoveRight => self.move_right = input,
            GameAction::Kick => self.kick = input,
            GameAction::CurveLeft => self.curve_left = input,
            GameAction::CurveRight => self.curve_right = input,
            GameAction::Wildcard => self.wildcard = input,
            GameAction::Sprint => self.sprint = input,
            GameAction::Mode => self.mode = input,
        }
    }
}

// Persistencia de gamepad bindings
pub fn get_gamepad_bindings_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join("gamepad_bindings.ron"))
}

pub fn load_gamepad_bindings() -> GamepadBindingsConfig {
    let Some(path) = get_gamepad_bindings_path() else {
        println!("[Config] No se pudo determinar la ruta de gamepad config, usando defaults");
        return GamepadBindingsConfig::default();
    };

    match fs::read_to_string(&path) {
        Ok(content) => match ron::from_str::<GamepadBindingsConfig>(&content) {
            Ok(config) => {
                println!("[Config] Gamepad bindings cargados desde {:?}", path);
                config
            }
            Err(e) => {
                println!(
                    "[Config] Error parseando gamepad bindings ({}), usando defaults",
                    e
                );
                GamepadBindingsConfig::default()
            }
        },
        Err(_) => {
            println!("[Config] No existe archivo de gamepad bindings, usando defaults");
            GamepadBindingsConfig::default()
        }
    }
}

pub fn save_gamepad_bindings(config: &GamepadBindingsConfig) -> Result<(), String> {
    let config_dir = get_config_dir().ok_or("No se pudo determinar directorio de config")?;

    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Error creando directorio de config: {}", e))?;

    let path = config_dir.join("gamepad_bindings.ron");

    let content = ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("Error serializando config: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Error escribiendo archivo: {}", e))?;

    println!("[Config] Gamepad bindings guardados en {:?}", path);
    Ok(())
}

// ============================================
// GilrsWrapper - Acceso thread-safe a gilrs
// ============================================

/// Wrapper thread-safe para gilrs
#[derive(Resource)]
pub struct GilrsWrapper {
    pub gilrs: Arc<Mutex<gilrs::Gilrs>>,
}

impl GilrsWrapper {
    pub fn new() -> Option<Self> {
        match gilrs::Gilrs::new() {
            Ok(g) => Some(Self {
                gilrs: Arc::new(Mutex::new(g)),
            }),
            Err(e) => {
                println!("⚠️ No se pudo inicializar gilrs: {}", e);
                None
            }
        }
    }
}

/// Estado actual de un gamepad leído por gilrs
#[derive(Default, Clone)]
pub struct RawGamepadState {
    pub buttons: [bool; 32],
    pub axes: [f32; 8],
}

impl RawGamepadState {
    /// Verifica si un input está activo
    pub fn is_active(&self, input: RawGamepadInput) -> bool {
        const AXIS_THRESHOLD: f32 = 0.3;
        match input {
            RawGamepadInput::Button(n) => self.buttons.get(n as usize).copied().unwrap_or(false),
            RawGamepadInput::AxisPositive(n) => {
                self.axes.get(n as usize).copied().unwrap_or(0.0) > AXIS_THRESHOLD
            }
            RawGamepadInput::AxisNegative(n) => {
                self.axes.get(n as usize).copied().unwrap_or(0.0) < -AXIS_THRESHOLD
            }
        }
    }
}

/// Evento detectado durante configuración
#[derive(Debug, Clone)]
pub enum DetectedInput {
    Keyboard(KeyCode),
    Gamepad(RawGamepadInput),
}

impl DetectedInput {
    pub fn display_name(&self) -> String {
        match self {
            DetectedInput::Keyboard(k) => format!("Tecla: {}", key_code_display_name(*k)),
            DetectedInput::Gamepad(g) => format!("Gamepad: {}", g.display_name()),
        }
    }
}

/// Estado de UI extendido para configuración de input
#[derive(Resource, Default)]
pub struct InputConfigUIState {
    /// Acción siendo configurada
    pub rebinding_action: Option<GameAction>,
    /// Tipo de dispositivo siendo configurado (0=teclado, 1=gamepad)
    pub device_tab: usize,
    /// Bindings pendientes de teclado
    pub pending_keyboard: Option<KeyBindingsConfig>,
    /// Bindings pendientes de gamepad
    pub pending_gamepad: Option<GamepadBindingsConfig>,
    /// Mensaje de estado
    pub status_message: Option<String>,
    /// Último input detectado (para mostrar feedback)
    pub last_detected: Option<DetectedInput>,
}

// ============================================
// GamepadBindingsMap - Bindings por tipo de gamepad
// ============================================

/// HashMap de bindings por tipo/nombre de gamepad
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct GamepadBindingsMap {
    pub bindings: HashMap<String, GamepadBindingsConfig>,
}

impl GamepadBindingsMap {
    /// Obtiene los bindings para un tipo de gamepad, o los defaults si no existe
    pub fn get_bindings(&self, gamepad_type: &str) -> GamepadBindingsConfig {
        self.bindings
            .get(gamepad_type)
            .cloned()
            .unwrap_or_default()
    }

    /// Establece los bindings para un tipo de gamepad
    pub fn set_bindings(&mut self, gamepad_type: String, bindings: GamepadBindingsConfig) {
        self.bindings.insert(gamepad_type, bindings);
    }
}

// ============================================
// GamepadConfigUIState - Estado de UI para configuración
// ============================================

/// Estado de UI para configuración de gamepad
#[derive(Resource, Default)]
pub struct GamepadConfigUIState {
    /// Índice del jugador que está configurando (para volver después)
    pub configuring_player_index: Option<usize>,
    /// Nombre/tipo del gamepad siendo configurado
    pub gamepad_type_name: Option<String>,
    /// Acción siendo rebindeada
    pub rebinding_action: Option<GameAction>,
    /// Bindings pendientes (copia editable)
    pub pending_bindings: Option<GamepadBindingsConfig>,
    /// Mensaje de estado
    pub status_message: Option<String>,
    /// Último input detectado (para mostrar feedback)
    pub last_detected_input: Option<RawGamepadInput>,
}

impl GamepadConfigUIState {
    /// Inicia la configuración para un gamepad
    pub fn start_config(
        &mut self,
        player_index: usize,
        gamepad_type: String,
        current_bindings: GamepadBindingsConfig,
    ) {
        self.configuring_player_index = Some(player_index);
        self.gamepad_type_name = Some(gamepad_type);
        self.pending_bindings = Some(current_bindings);
        self.rebinding_action = None;
        self.status_message = None;
        self.last_detected_input = None;
    }

    /// Inicia el rebinding de una acción
    pub fn start_rebind(&mut self, action: GameAction) {
        println!(
            "🎮 [GamepadConfig] Iniciando rebind para '{}'",
            action.display_name()
        );
        self.rebinding_action = Some(action);
        self.status_message = Some(format!(
            "Presiona un botón/eje para '{}'... (ESC para cancelar)",
            action.display_name()
        ));
    }

    /// Cancela el rebinding actual
    pub fn cancel_rebind(&mut self) {
        println!("🎮 [GamepadConfig] Rebind cancelado");
        self.rebinding_action = None;
        self.status_message = None;
    }

    /// Verifica si está en modo rebinding
    pub fn is_rebinding(&self) -> bool {
        self.rebinding_action.is_some()
    }

    /// Reinicia el estado
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ============================================
// DetectedGamepadEvent - Evento detectado durante rebinding
// ============================================

/// Evento de gamepad detectado durante configuración
#[derive(Resource, Default)]
pub struct DetectedGamepadEvent {
    pub input: Option<(gilrs::GamepadId, RawGamepadInput)>,
}

// ============================================
// Persistencia de GamepadBindingsMap
// ============================================

pub fn get_gamepad_bindings_map_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join("gamepad_bindings_map.ron"))
}

pub fn load_gamepad_bindings_map() -> GamepadBindingsMap {
    let Some(path) = get_gamepad_bindings_map_path() else {
        println!("[Config] No se pudo determinar la ruta de gamepad bindings map, usando defaults");
        return GamepadBindingsMap::default();
    };

    match fs::read_to_string(&path) {
        Ok(content) => match ron::from_str::<GamepadBindingsMap>(&content) {
            Ok(map) => {
                println!(
                    "[Config] Gamepad bindings map cargado desde {:?} ({} entradas)",
                    path,
                    map.bindings.len()
                );
                map
            }
            Err(e) => {
                println!(
                    "[Config] Error parseando gamepad bindings map ({}), usando defaults",
                    e
                );
                GamepadBindingsMap::default()
            }
        },
        Err(_) => {
            println!("[Config] No existe archivo de gamepad bindings map, usando defaults");
            GamepadBindingsMap::default()
        }
    }
}

pub fn save_gamepad_bindings_map(map: &GamepadBindingsMap) -> Result<(), String> {
    let config_dir = get_config_dir().ok_or("No se pudo determinar directorio de config")?;

    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Error creando directorio de config: {}", e))?;

    let path = config_dir.join("gamepad_bindings_map.ron");

    let content = ron::ser::to_string_pretty(map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("Error serializando config: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Error escribiendo archivo: {}", e))?;

    println!("[Config] Gamepad bindings map guardado en {:?}", path);
    Ok(())
}

// ============================================
// Helpers para convertir gilrs Button/Axis a índices (públicos)
// ============================================

/// Convierte un gilrs::Button a su índice numérico
pub fn gilrs_button_to_idx(button: gilrs::Button) -> Option<u8> {
    use gilrs::Button::*;
    match button {
        South => Some(0),
        East => Some(1),
        North => Some(2),
        West => Some(3),
        C => Some(4),
        Z => Some(5),
        LeftTrigger => Some(6),
        LeftTrigger2 => Some(7),
        RightTrigger => Some(8),
        RightTrigger2 => Some(9),
        Select => Some(10),
        Start => Some(11),
        Mode => Some(12),
        LeftThumb => Some(13),
        RightThumb => Some(14),
        DPadUp => Some(15),
        DPadDown => Some(16),
        DPadLeft => Some(17),
        DPadRight => Some(18),
        Unknown => None,
    }
}

/// Convierte un código de botón gilrs a índice, con fallback para botones desconocidos
pub fn gilrs_button_code_to_idx(button: gilrs::Button, code: gilrs::ev::Code) -> u8 {
    // Primero intentar el mapeo estándar
    if let Some(idx) = gilrs_button_to_idx(button) {
        return idx;
    }

    // Para botones desconocidos, usar el código raw
    // Los códigos de botones de joystick empiezan en 288 (BTN_TRIGGER)
    // Mapeamos: 288 -> 0, 289 -> 1, etc.
    let raw_code: u32 = code.into_u32();
    if raw_code >= 288 && raw_code < 320 {
        (raw_code - 288) as u8
    } else if raw_code >= 304 && raw_code < 320 {
        // BTN_A, BTN_B, etc. (gamepad buttons)
        (raw_code - 304) as u8
    } else {
        // Fallback: usar los últimos bits del código
        (raw_code & 0x1F) as u8
    }
}

/// Convierte un gilrs::Axis a su índice numérico
pub fn gilrs_axis_to_idx(axis: gilrs::Axis) -> Option<u8> {
    use gilrs::Axis::*;
    match axis {
        LeftStickX => Some(0),
        LeftStickY => Some(1),
        LeftZ => Some(2),
        RightStickX => Some(3),
        RightStickY => Some(4),
        RightZ => Some(5),
        DPadX => Some(6),
        DPadY => Some(7),
        Unknown => None,
    }
}
