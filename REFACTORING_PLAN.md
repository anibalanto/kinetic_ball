# Plan de Refactorización de main.rs

## Situación Actual

**Archivo:** `kinetic_ball/src/main.rs`
- **4,699 líneas** de código
- Contiene: componentes, recursos, sistemas de UI, networking, cámaras, renderizado, input

---

## Estructura Propuesta

```
src/
├── main.rs              (~200 líneas - solo setup de App y plugins)
├── states.rs            (AppState, estados de la app)
├── components.rs        (todos los Component)
├── resources.rs         (todos los Resource)
├── assets.rs            (EmbeddedAssets, carga de assets)
├── color_utils.rs       (funciones de colores)
├── ui/
│   ├── mod.rs
│   ├── menu.rs
│   ├── settings.rs
│   ├── room_selection.rs
│   ├── create_room.rs
│   ├── hosting.rs
│   ├── local_players_setup.rs
│   └── gamepad_config.rs
├── networking/
│   ├── mod.rs
│   ├── client.rs
│   └── messages.rs
├── camera/
│   ├── mod.rs
│   ├── follow.rs
│   └── split_screen.rs
├── rendering/
│   ├── mod.rs
│   ├── field.rs
│   ├── minimap.rs
│   └── player_visuals.rs
├── game/
│   ├── mod.rs
│   ├── setup.rs
│   ├── input.rs
│   └── interpolation.rs
├── keybindings.rs       (existente)
├── local_players.rs     (existente)
├── host/                (existente)
└── shared/              (existente)
```

---

## Fases de Implementación

### Fase 1: Tipos base
- `states.rs` - AppState, RoomStatus, RoomInfo
- `color_utils.rs` - funciones de colores
- `assets.rs` - constantes EMBEDDED_* y EmbeddedAssets

### Fase 2: Componentes y recursos
- `components.rs` - todos los #[derive(Component)]
- `resources.rs` - todos los #[derive(Resource)]

### Fase 3: UI
- `ui/mod.rs` + 7 submódulos

### Fase 4: Networking
- `networking/mod.rs` + client.rs + messages.rs

### Fase 5: Camera
- `camera/mod.rs` + follow.rs + split_screen.rs

### Fase 6: Rendering
- `rendering/mod.rs` + field.rs + minimap.rs + player_visuals.rs

### Fase 7: Game
- `game/mod.rs` + setup.rs + input.rs + interpolation.rs

### Fase 8: Limpiar main.rs
