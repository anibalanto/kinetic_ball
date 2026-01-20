# Implementación de Keybindings Configurables

## Estado Actual

### Completado
- [x] `client/Cargo.toml` - Dependencias `ron` y `dirs` agregadas
- [x] `client/src/keybindings.rs` - Módulo completo creado
- [x] `client/src/main.rs` - AppState con Settings agregado
- [x] `client/src/main.rs` - Imports del módulo keybindings agregados
- [x] Registrar recursos en main (load_keybindings() y SettingsUIState)
- [x] Agregar sistema settings_ui
- [x] Modificar menu_ui (botón "Teclas")
- [x] Modificar handle_input (usar keybindings configurables)

### Pendiente
(Ninguno - Implementación completa)

---

## Cambios Pendientes en `client/src/main.rs`

### 1. Registrar Recursos de Keybindings

**Ubicación:** Después de línea ~190 (después de `DoubleTapTracker`)

```rust
        .insert_resource(DoubleTapTracker {
            last_space_press: -999.0,
        })
        // ========== AGREGAR ESTO ==========
        // Keybindings configurables
        .insert_resource(load_keybindings())
        .insert_resource(SettingsUIState::default())
        // ==================================
        // Cargar assets embebidos al inicio (antes de todo)
        .add_systems(Startup, load_embedded_assets)
```

### 2. Agregar Sistema settings_ui

**Ubicación:** Después de línea ~195 (después de `menu_ui`)

```rust
        // Sistemas de menú (solo en estado Menu)
        .add_systems(OnEnter(AppState::Menu), setup_menu_camera)
        .add_systems(Update, menu_ui.run_if(in_state(AppState::Menu)))
        // ========== AGREGAR ESTO ==========
        // Sistemas de configuración (solo en estado Settings)
        .add_systems(OnEnter(AppState::Settings), setup_menu_camera)
        .add_systems(Update, settings_ui.run_if(in_state(AppState::Settings)))
        // ==================================
        // Sistema de conexión (solo en estado Connecting)
```

### 3. Agregar Función `settings_ui`

**Ubicación:** Después de la función `menu_ui` (aproximadamente línea 410)

```rust
/// Sistema de UI para configuración de teclas
fn settings_ui(
    mut contexts: EguiContexts,
    mut keybindings: ResMut<KeyBindingsConfig>,
    mut ui_state: ResMut<SettingsUIState>,
    mut next_state: ResMut<NextState<AppState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Inicializar pending_bindings si es necesario
    if ui_state.pending_bindings.is_none() {
        ui_state.pending_bindings = Some(keybindings.clone());
    }

    // Capturar tecla si estamos en modo rebind
    if let Some(action) = ui_state.rebinding_action {
        for key in keyboard.get_just_pressed() {
            if *key == KeyCode::Escape {
                ui_state.cancel_rebind();
            } else {
                if let Some(ref mut pending) = ui_state.pending_bindings {
                    pending.set_key(action, *key);
                }
                ui_state.rebinding_action = None;
                ui_state.status_message = Some(format!(
                    "'{}' asignado a {}",
                    action.display_name(),
                    key_code_display_name(*key)
                ));
            }
            break;
        }
    }

    egui::CentralPanel::default().show(contexts.ctx_mut(), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(egui::RichText::new("Configuración de Teclas").size(36.0));
            ui.add_space(20.0);

            // Mensaje de estado
            if let Some(ref msg) = ui_state.status_message {
                ui.label(
                    egui::RichText::new(msg)
                        .size(16.0)
                        .color(egui::Color32::YELLOW),
                );
                ui.add_space(10.0);
            }

            // Grid de keybindings
            egui::Frame::none()
                .inner_margin(20.0)
                .show(ui, |ui| {
                    egui::Grid::new("keybindings_grid")
                        .num_columns(2)
                        .spacing([40.0, 8.0])
                        .show(ui, |ui| {
                            let pending = ui_state
                                .pending_bindings
                                .as_ref()
                                .unwrap_or(&keybindings);

                            for action in GameAction::all() {
                                // Nombre de la acción
                                ui.label(
                                    egui::RichText::new(action.display_name()).size(18.0),
                                );

                                // Botón con tecla actual
                                let key = pending.get_key(*action);
                                let is_rebinding =
                                    ui_state.rebinding_action == Some(*action);

                                let button_text = if is_rebinding {
                                    "Presiona una tecla...".to_string()
                                } else {
                                    key_code_display_name(key)
                                };

                                let button = egui::Button::new(
                                    egui::RichText::new(&button_text).size(16.0),
                                );

                                if ui.add_sized([150.0, 28.0], button).clicked()
                                    && !ui_state.is_rebinding()
                                {
                                    ui_state.start_rebind(*action);
                                }

                                ui.end_row();
                            }
                        });
                });

            ui.add_space(30.0);

            // Botones de acción
            ui.horizontal(|ui| {
                // Guardar
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Guardar").size(18.0)),
                    )
                    .clicked()
                {
                    if let Some(ref pending) = ui_state.pending_bindings {
                        *keybindings = pending.clone();
                        match save_keybindings(&keybindings) {
                            Ok(_) => {
                                ui_state.status_message =
                                    Some("Configuración guardada".to_string());
                            }
                            Err(e) => {
                                ui_state.status_message =
                                    Some(format!("Error al guardar: {}", e));
                            }
                        }
                    }
                }

                ui.add_space(15.0);

                // Restaurar defaults
                if ui
                    .add_sized(
                        [180.0, 40.0],
                        egui::Button::new(
                            egui::RichText::new("Restaurar Defaults").size(18.0),
                        ),
                    )
                    .clicked()
                {
                    ui_state.pending_bindings = Some(KeyBindingsConfig::default());
                    ui_state.status_message =
                        Some("Restaurado a valores por defecto".to_string());
                }

                ui.add_space(15.0);

                // Volver
                if ui
                    .add_sized(
                        [120.0, 40.0],
                        egui::Button::new(egui::RichText::new("Volver").size(18.0)),
                    )
                    .clicked()
                {
                    ui_state.rebinding_action = None;
                    ui_state.pending_bindings = None;
                    ui_state.status_message = None;
                    next_state.set(AppState::Menu);
                }
            });
        });
    });
}
```

### 4. Modificar `menu_ui` - Agregar Botón Settings

**Ubicación:** Dentro de la función `menu_ui`, después del botón "Conectar"

Buscar este código (aproximadamente línea 400):
```rust
            // Botón Conectar
            ui.add_space(30.0);
            if ui
                .add_sized(
                    [200.0, 50.0],
                    egui::Button::new(egui::RichText::new("Conectar").size(20.0)),
                )
                .clicked()
            {
                next_state.set(AppState::Connecting);
            }
```

Reemplazar por:
```rust
            // Botones
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(40.0);

                // Botón Conectar
                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Conectar").size(20.0)),
                    )
                    .clicked()
                {
                    next_state.set(AppState::Connecting);
                }

                ui.add_space(20.0);

                // Botón Configuración
                if ui
                    .add_sized(
                        [150.0, 50.0],
                        egui::Button::new(egui::RichText::new("Teclas").size(20.0)),
                    )
                    .clicked()
                {
                    next_state.set(AppState::Settings);
                }
            });
```

### 5. Modificar `handle_input` - Usar Keybindings

**Ubicación:** Función `handle_input` (aproximadamente línea 720)

**Cambio en la firma de la función:**
```rust
fn handle_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    channels: Res<NetworkChannels>,
    my_player_id: Res<MyPlayerId>,
    mut previous_input: ResMut<PreviousInput>,
    mut double_tap: ResMut<DoubleTapTracker>,
    time: Res<Time>,
    keybindings: Res<KeyBindingsConfig>,  // <-- AGREGAR ESTE PARÁMETRO
) {
```

**Cambio en la detección de double-tap:**
```rust
    // ANTES:
    if keyboard.just_pressed(KeyCode::Space) {

    // DESPUÉS:
    if keyboard.just_pressed(keybindings.sprint.0) {
```

**Cambio en la construcción de PlayerInput:**
```rust
    // ANTES:
    let input = PlayerInput {
        move_up: keyboard.pressed(KeyCode::ArrowUp),
        move_down: keyboard.pressed(KeyCode::ArrowDown),
        move_left: keyboard.pressed(KeyCode::ArrowLeft),
        move_right: keyboard.pressed(KeyCode::ArrowRight),
        kick: keyboard.pressed(KeyCode::KeyS),
        curve_left: keyboard.pressed(KeyCode::KeyD),
        curve_right: keyboard.pressed(KeyCode::KeyA),
        stop_interact: keyboard.pressed(KeyCode::ShiftLeft),
        sprint: keyboard.pressed(KeyCode::Space),
        dash: dash_detected,
        slide: keyboard.pressed(KeyCode::ControlLeft),
    };

    // DESPUÉS:
    let input = PlayerInput {
        move_up: keyboard.pressed(keybindings.move_up.0),
        move_down: keyboard.pressed(keybindings.move_down.0),
        move_left: keyboard.pressed(keybindings.move_left.0),
        move_right: keyboard.pressed(keybindings.move_right.0),
        kick: keyboard.pressed(keybindings.kick.0),
        curve_left: keyboard.pressed(keybindings.curve_left.0),
        curve_right: keyboard.pressed(keybindings.curve_right.0),
        stop_interact: keyboard.pressed(keybindings.stop_interact.0),
        sprint: keyboard.pressed(keybindings.sprint.0),
        dash: dash_detected,
        slide: keyboard.pressed(keybindings.slide.0),
    };
```

---

## Archivo de Configuración Generado

Después de guardar en el menú, se creará el archivo:
- **Linux:** `~/.config/rustball/keybindings.ron`
- **macOS:** `~/Library/Application Support/rustball/keybindings.ron`
- **Windows:** `%APPDATA%\rustball\keybindings.ron`

Contenido de ejemplo:
```ron
(
    move_up: "ArrowUp",
    move_down: "ArrowDown",
    move_left: "ArrowLeft",
    move_right: "ArrowRight",
    kick: "KeyS",
    curve_left: "KeyD",
    curve_right: "KeyA",
    stop_interact: "ShiftLeft",
    sprint: "Space",
    slide: "ControlLeft",
)
```

---

## Verificación

1. `cargo build -p client`
2. `cargo run -p client`
3. Click en "Teclas" en el menú principal
4. Cambiar alguna tecla y guardar
5. Verificar que el archivo se creó en la ruta correspondiente
6. Reiniciar el cliente y verificar que carga la configuración
