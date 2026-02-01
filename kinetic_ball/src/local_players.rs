use crate::keybindings::GameAction; // Importación directa
use crate::keybindings::{GamepadBindingsConfig, GamepadBindingsMap, GilrsWrapper, RawGamepadInput};
use bevy::prelude::*;

// ============================================================================
// TIPOS DE DISPOSITIVO DE INPUT
// ============================================================================

/// Representa un dispositivo de entrada para un jugador local
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputDevice {
    Keyboard,
    Gamepad(Entity),
    /// Gamepad genérico leído a través de `gilrs`
    RawGamepad(gilrs::GamepadId),
}

impl InputDevice {
    pub fn display_name(&self, available: &AvailableInputDevices) -> String {
        match self {
            InputDevice::Keyboard => "Teclado".to_string(),
            InputDevice::Gamepad(entity) => available
                .gamepads
                .iter()
                .find(|(e, _)| e == entity)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("Gamepad {:?}", entity)),
            InputDevice::RawGamepad(id) => available
                .raw_gamepads
                .iter()
                .find(|(g_id, _)| g_id == id)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("Gamepad Genérico {:?}", id)),
        }
    }
}

// ============================================================================
// JUGADOR LOCAL
// ============================================================================

/// Representa un jugador local con su dispositivo de entrada asignado
#[derive(Debug, Clone)]
pub struct LocalPlayer {
    /// Índice local (0, 1, 2, 3)
    pub local_index: u8,
    /// Nombre del jugador
    pub name: String,
    /// Dispositivo de entrada asignado
    pub input_device: InputDevice,
    /// ID del jugador asignado por el servidor (después del JOIN)
    pub server_player_id: Option<u32>,
    /// Nombre/tipo del gamepad (para buscar bindings en GamepadBindingsMap)
    pub gamepad_type_name: Option<String>,
}

impl LocalPlayer {
    pub fn new(
        local_index: u8,
        name: String,
        input_device: InputDevice,
        gamepad_type_name: Option<String>,
    ) -> Self {
        Self {
            local_index,
            name,
            input_device,
            server_player_id: None,
            gamepad_type_name,
        }
    }
}

// ============================================================================
// RECURSOS
// ============================================================================

/// Lista de jugadores locales configurados
#[derive(Resource, Default)]
pub struct LocalPlayers {
    pub players: Vec<LocalPlayer>,
    /// Máximo de jugadores locales permitidos
    pub max_players: u8,
}

impl LocalPlayers {
    pub fn new(max_players: u8) -> Self {
        Self {
            players: Vec::new(),
            max_players,
        }
    }

    pub fn add_player(
        &mut self,
        name: String,
        device: InputDevice,
        gilrs: Option<&GilrsWrapper>,
    ) -> Result<u8, &'static str> {
        if self.players.len() >= self.max_players as usize {
            return Err("Máximo de jugadores locales alcanzado");
        }

        // Verificar que el dispositivo no esté ya en uso
        if self.players.iter().any(|p| p.input_device == device) {
            return Err("Este dispositivo ya está en uso");
        }

        // Obtener el nombre del tipo de gamepad si es RawGamepad
        let gamepad_type_name = if let InputDevice::RawGamepad(id) = &device {
            if let Some(gilrs_wrapper) = gilrs {
                if let Ok(gilrs_instance) = gilrs_wrapper.gilrs.lock() {
                    gilrs_instance
                        .connected_gamepad(*id)
                        .map(|g| g.name().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let local_index = self.players.len() as u8;
        self.players
            .push(LocalPlayer::new(local_index, name, device, gamepad_type_name));
        Ok(local_index)
    }

    pub fn remove_player(&mut self, local_index: u8) {
        self.players.retain(|p| p.local_index != local_index);
        // Re-indexar
        for (i, player) in self.players.iter_mut().enumerate() {
            player.local_index = i as u8;
        }
    }

    pub fn get_by_server_id(&self, server_id: u32) -> Option<&LocalPlayer> {
        self.players
            .iter()
            .find(|p| p.server_player_id == Some(server_id))
    }

    pub fn get_by_server_id_mut(&mut self, server_id: u32) -> Option<&mut LocalPlayer> {
        self.players
            .iter_mut()
            .find(|p| p.server_player_id == Some(server_id))
    }

    pub fn is_device_available(&self, device: &InputDevice) -> bool {
        !self.players.iter().any(|p| &p.input_device == device)
    }

    pub fn count(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }
}

/// Dispositivos de entrada disponibles (detectados)
#[derive(Resource, Default)]
pub struct AvailableInputDevices {
    /// Gamepads estándar de Bevy: (Entity, nombre descriptivo)
    pub gamepads: Vec<(Entity, String)>,
    /// Gamepads genéricos de gilrs: (ID, nombre descriptivo)
    pub raw_gamepads: Vec<(gilrs::GamepadId, String)>,
}

impl AvailableInputDevices {
    /// Obtiene la lista de dispositivos disponibles para selección
    /// Si hay gamepads gilrs disponibles, no mostramos los de Bevy para evitar duplicados
    pub fn get_available_devices(
        &self,
        local_players: &LocalPlayers,
    ) -> Vec<(InputDevice, String)> {
        let mut devices = Vec::new();

        // Teclado siempre disponible si no está en uso
        let keyboard = InputDevice::Keyboard;
        if local_players.is_device_available(&keyboard) {
            devices.push((keyboard, "Teclado".to_string()));
        }

        // Si hay gamepads gilrs, usamos esos (tienen mejor soporte para genéricos)
        // y no mostramos los de Bevy para evitar duplicados
        if !self.raw_gamepads.is_empty() {
            for (id, name) in &self.raw_gamepads {
                let raw_gamepad = InputDevice::RawGamepad(*id);
                if local_players.is_device_available(&raw_gamepad) {
                    devices.push((raw_gamepad, name.clone()));
                }
            }
        } else {
            // Solo mostrar gamepads de Bevy si no hay gilrs disponible
            for (entity, name) in &self.gamepads {
                let gamepad = InputDevice::Gamepad(*entity);
                if local_players.is_device_available(&gamepad) {
                    devices.push((gamepad, name.clone()));
                }
            }
        }

        devices
    }
}

/// Estado de la UI de configuración de jugadores locales
#[derive(Resource, Default)]
pub struct LocalPlayersUIState {
    /// Nombre del nuevo jugador a agregar
    pub new_player_name: String,
    /// Índice del dispositivo seleccionado en la lista de disponibles
    pub selected_device_index: usize,
    /// Mensaje de error o estado
    pub status_message: Option<String>,
    /// Mostrar panel de configuración del servidor
    pub show_server_config: bool,
}

// ============================================================================
// SISTEMAS
// ============================================================================

/// Sistema que detecta la conexión y desconexión de gamepads
pub fn detect_gamepads(
    bevy_gamepads: Query<(Entity, &Gamepad)>,
    gilrs: Option<Res<GilrsWrapper>>,
    mut available: ResMut<AvailableInputDevices>,
) {
    let mut changed = false;

    // --- Detectar gamepads estándar de Bevy ---
    let current_bevy_gamepads: Vec<(Entity, String)> = bevy_gamepads
        .iter()
        .enumerate()
        .map(|(idx, (entity, _))| (entity, format!("Gamepad {}", idx + 1)))
        .collect();

    if current_bevy_gamepads != available.gamepads {
        available.gamepads = current_bevy_gamepads;
        changed = true;
    }

    // --- Detectar gamepads con gilrs ---
    if let Some(gilrs_wrapper) = gilrs {
        if let Ok(gilrs_instance) = gilrs_wrapper.gilrs.lock() {
            let current_raw_gamepads: Vec<(gilrs::GamepadId, String)> = gilrs_instance
                .gamepads()
                .map(|(id, gamepad)| (id, format!("{} (G)", gamepad.name())))
                .collect();

            if current_raw_gamepads != available.raw_gamepads {
                available.raw_gamepads = current_raw_gamepads;
                changed = true;
            }
        }
    }

    if changed {
        println!(
            "🎮 Dispositivos actualizados: {} Bevy, {} Gilrs",
            available.gamepads.len(),
            available.raw_gamepads.len()
        );
    }
}

// ============================================================================
// FUNCIONES DE LECTURA DE INPUT POR DISPOSITIVO
// ============================================================================

use crate::keybindings::KeyBindingsConfig;
use crate::shared::protocol::PlayerInput;

/// Lee el input del teclado
pub fn read_keyboard_input(
    keyboard: &ButtonInput<KeyCode>,
    keybindings: &KeyBindingsConfig,
    is_cube_mode: bool,
) -> PlayerInput {
    let modifier = keyboard.pressed(KeyCode::ControlLeft);
    let wildcard_pressed = keyboard.pressed(keybindings.wildcard.0);

    PlayerInput {
        move_up: keyboard.pressed(keybindings.move_up.0),
        move_down: keyboard.pressed(keybindings.move_down.0),
        move_left: keyboard.pressed(keybindings.move_left.0),
        move_right: keyboard.pressed(keybindings.move_right.0),
        kick: keyboard.pressed(keybindings.kick.0) && !modifier,
        curve_left: keyboard.pressed(keybindings.curve_left.0),
        curve_right: keyboard.pressed(keybindings.curve_right.0),
        stop_interact: wildcard_pressed && !is_cube_mode,
        dash: wildcard_pressed && is_cube_mode,
        sprint: keyboard.pressed(keybindings.sprint.0) && !modifier,
        mode: keyboard.pressed(keybindings.mode.0),
    }
}

/// Lee el input de un gamepad estándar de Bevy
pub fn read_bevy_gamepad_input(
    gamepad_entity: Entity,
    gamepads: &Query<&Gamepad>,
    is_cube_mode: bool,
) -> PlayerInput {
    let Ok(gamepad) = gamepads.get(gamepad_entity) else {
        return PlayerInput::default();
    };

    // Leer joystick izquierdo para movimiento
    let left_stick_x = gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
    let left_stick_y = gamepad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);

    // Deadzone
    const DEADZONE: f32 = 0.2;
    let move_left = left_stick_x < -DEADZONE;
    let move_right = left_stick_x > DEADZONE;
    let move_up = left_stick_y > DEADZONE;
    let move_down = left_stick_y < -DEADZONE;

    // Botones principales
    let kick = gamepad.pressed(GamepadButton::South); // A/Cross
    let curve_right = gamepad.pressed(GamepadButton::East); // B/Circle
    let curve_left = gamepad.pressed(GamepadButton::West); // X/Square
    let mode = gamepad.pressed(GamepadButton::North); // Y/Triangle

    // Triggers y hombros
    let left_trigger_axis = gamepad.get(GamepadAxis::LeftZ).unwrap_or(0.0);
    let right_trigger_axis = gamepad.get(GamepadAxis::RightZ).unwrap_or(0.0);
    let left_shoulder = gamepad.pressed(GamepadButton::LeftTrigger);
    let right_shoulder = gamepad.pressed(GamepadButton::RightTrigger);
    let left_trigger_btn = gamepad.pressed(GamepadButton::LeftTrigger2);
    let right_trigger_btn = gamepad.pressed(GamepadButton::RightTrigger2);

    const TRIGGER_THRESHOLD: f32 = 0.1;

    let wildcard_pressed =
        left_trigger_axis > TRIGGER_THRESHOLD || left_trigger_btn || left_shoulder;
    let sprint = right_trigger_axis > TRIGGER_THRESHOLD || right_trigger_btn || right_shoulder;

    PlayerInput {
        move_up,
        move_down,
        move_left,
        move_right,
        kick,
        curve_left,
        curve_right,
        stop_interact: wildcard_pressed && !is_cube_mode,
        dash: wildcard_pressed && is_cube_mode,
        sprint,
        mode,
    }
}

/// Lee el input de un gamepad genérico usando `gilrs` y la configuración de bindings
// En kinetic_ball/src/local_players.rs

fn read_raw_gamepad_input(
    gamepad: gilrs::Gamepad<'_>,
    gamepad_bindings: &GamepadBindingsConfig,
    is_cube_mode: bool,
) -> PlayerInput {
    let mut input = PlayerInput::default();
    input.mode = is_cube_mode;

    // Recopilar qué botones están presionados (por su índice según nuestro mapeo)
    let mut pressed_buttons: Vec<u8> = Vec::new();
    for (code, data) in gamepad.state().buttons() {
        if data.is_pressed() {
            // Convertir el código del botón a nuestro índice
            let raw_code: u32 = code.into_u32();
            let idx = if raw_code >= 288 && raw_code < 320 {
                // BTN_TRIGGER range (generic joysticks)
                (raw_code - 288) as u8
            } else if raw_code >= 304 && raw_code < 320 {
                // BTN_GAMEPAD/BTN_SOUTH range (standard gamepads)
                (raw_code - 304) as u8
            } else {
                (raw_code & 0x1F) as u8
            };
            pressed_buttons.push(idx);
        }
    }

    // Helper para verificar si un binding está activo
    let is_binding_active = |binding: &Option<RawGamepadInput>| -> bool {
        if let Some(b) = binding {
            match b {
                RawGamepadInput::Button(idx) => pressed_buttons.contains(idx),
                RawGamepadInput::AxisPositive(idx) => {
                    idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) > 0.5)
                }
                RawGamepadInput::AxisNegative(idx) => {
                    idx_to_gilrs_axis(*idx as usize).map_or(false, |ax| gamepad.value(ax) < -0.5)
                }
            }
        } else {
            false
        }
    };

    // Mapeo con los nombres reales de tus campos en GamepadBindingsConfig
    if is_binding_active(&gamepad_bindings.move_up) {
        input.move_up = true;
    }
    if is_binding_active(&gamepad_bindings.move_down) {
        input.move_down = true;
    }
    if is_binding_active(&gamepad_bindings.move_left) {
        input.move_left = true;
    }
    if is_binding_active(&gamepad_bindings.move_right) {
        input.move_right = true;
    }
    if is_binding_active(&gamepad_bindings.kick) {
        input.kick = true;
    }
    if is_binding_active(&gamepad_bindings.sprint) {
        input.sprint = true;
    }
    if is_binding_active(&gamepad_bindings.mode) {
        input.mode = !is_cube_mode;
    }
    if is_binding_active(&gamepad_bindings.curve_left) {
        input.curve_left = true;
    }
    if is_binding_active(&gamepad_bindings.curve_right) {
        input.curve_right = true;
    }
    if is_binding_active(&gamepad_bindings.wildcard) {
        input.stop_interact = !is_cube_mode;
        input.dash = is_cube_mode;
    }

    input
}

fn idx_to_gilrs_button(idx: usize) -> Option<gilrs::Button> {
    use gilrs::Button::*;
    match idx {
        0 => Some(South),
        1 => Some(East),
        2 => Some(North),
        3 => Some(West),
        4 => Some(LeftTrigger),
        5 => Some(RightTrigger),
        6 => Some(LeftTrigger2),
        7 => Some(RightTrigger2),
        8 => Some(Select),
        9 => Some(Start),
        10 => Some(Mode),
        11 => Some(LeftThumb),
        12 => Some(RightThumb),
        13 => Some(DPadUp),
        14 => Some(DPadDown),
        15 => Some(DPadLeft),
        16 => Some(DPadRight),
        _ => None,
    }
}

pub fn idx_to_gilrs_axis(idx: usize) -> Option<gilrs::Axis> {
    use gilrs::Axis::*;
    match idx {
        0 => Some(LeftStickX),
        1 => Some(LeftStickY),
        2 => Some(LeftZ),
        3 => Some(RightStickX),
        4 => Some(RightStickY),
        5 => Some(RightZ),
        6 => Some(DPadX),
        7 => Some(DPadY),
        _ => None,
    }
}

// Helpers para convertir de gilrs a nuestros índices
fn gilrs_button_to_idx(button: gilrs::Button) -> Option<usize> {
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

fn gilrs_axis_to_idx(axis: gilrs::Axis) -> Option<usize> {
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

/// Lee el input de un jugador local según su dispositivo asignado
pub fn read_local_player_input(
    player: &LocalPlayer,
    keyboard: &ButtonInput<KeyCode>,
    keybindings: &KeyBindingsConfig,
    gamepads: &Query<&Gamepad>,
    gilrs: Option<&GilrsWrapper>,
    gamepad_bindings_map: &GamepadBindingsMap,
    is_cube_mode: bool,
) -> PlayerInput {
    match &player.input_device {
        InputDevice::Keyboard => read_keyboard_input(keyboard, keybindings, is_cube_mode),
        InputDevice::Gamepad(entity) => read_bevy_gamepad_input(*entity, gamepads, is_cube_mode),
        InputDevice::RawGamepad(id) => {
            if let Some(gilrs_wrapper) = gilrs {
                if let Ok(gilrs_instance) = gilrs_wrapper.gilrs.lock() {
                    // Obtener el gamepad conectado por su ID
                    if let Some(gamepad) = gilrs_instance.connected_gamepad(*id) {
                        // Obtener bindings específicos para este tipo de gamepad
                        let gamepad_bindings = if let Some(ref type_name) = player.gamepad_type_name
                        {
                            gamepad_bindings_map.get_bindings(type_name)
                        } else {
                            GamepadBindingsConfig::default()
                        };
                        return read_raw_gamepad_input(gamepad, &gamepad_bindings, is_cube_mode);
                    }
                }
            }
            PlayerInput::default()
        }
    }
}
