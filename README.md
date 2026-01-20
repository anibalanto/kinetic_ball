<p align="center">
  <img src="images/logo.png" alt="kinetic_ball" width="400">
</p>

# kinetic_ball

Inspired by seeing how much could be done with just an X in HaxBall, written in Rust using [Bevy](https://bevyengine.org/) as the game engine and [Matchbox](https://github.com/johanhelsing/matchbox) for WebRTC peer-to-peer networking.

## Features

- Authoritative server with Rapier2D physics
- Graphical client with Bevy 0.17
- WebRTC peer-to-peer networking via matchbox_socket
- Custom map support (HaxBall format `.hbs`, `.json`, `.json5`)
- Configurable keybindings (saved in `~/.config/rustball/keybindings.ron`)
- Minimap and player detail camera
- Kick system with curve/spin effect
- Sprint, slide and cube mode

## Requirements

- Rust 1.75+
- `matchbox_server` for WebRTC signaling

```bash
cargo install matchbox_server
```

## Building

```bash
# Build everything (server + client + shared)
cargo build --release

# Or build separately
cargo build --release -p server
cargo build --release -p client
```

## How to Play

### Local Game (same machine)

1. **Start the matchbox signaling server:**
   ```bash
   matchbox_server
   ```
   This starts the signaling server at `ws://127.0.0.1:3536`

2. **Start the game server:**
   ```bash
   cargo run --release -p server
   ```
   Useful options:
   ```bash
   # With a custom map
   cargo run --release -p server -- --map maps/futsal_fah.hbs

   # List available maps
   cargo run --release -p server -- --list-maps

   # Scale the map
   cargo run --release -p server -- --map maps/cancha_grande.json5 --scale 1.5
   ```

3. **Start the client:**
   ```bash
   cargo run --release -p client -- --name YourName
   ```

### Online Play with ngrok

To play with friends over the internet, you need to expose the signaling server using [ngrok](https://ngrok.com/):

1. **Start matchbox_server:**
   ```bash
   matchbox_server
   ```

2. **Expose with ngrok (in another terminal):**
   ```bash
   ngrok http 3536
   ```
   ngrok will give you a URL like `https://xxxxxxxxxxxx.ngrok-free.app`

3. **Start the game server pointing to ngrok:**
   ```bash
   cargo run --release -p server -- \
     --signaling-url wss://xxxxxxxxxxxx.ngrok-free.app \
     --room my_room
   ```

4. **Clients connect using the same URL:**
   ```bash
   cargo run --release -p client -- \
     --server wss://xxxxxxxxxxxx.ngrok-free.app \
     --room my_room \
     --name Player1
   ```

**Note:** The host running the server can also connect as a client.

## Controls

![Keyboard controls](images/keyboard.png)

### Sphere Mode
Default mode that allows ball control and kicking

| Action | Default Key |
|--------|-------------|
| Move | Arrow keys |
| Kick | S |
| Curve left | A |
| Curve right | D |
| Sprint/Run | Space |
| Don't touch ball | Shift |

### Cube Mode (Right Ctrl)
Allows sliding and dribbling, always runs and doesn't interact with the ball without performing an action.
When stamina runs out, it automatically returns to sphere mode.

| Action | Default Key |
|--------|-------------|
| Slide | S |
| Slide right | D |
| Slide left | A |
| Direction change | Space + arrows |

### Settings
| Action | Key |
|--------|-----|
| Camera zoom | Keys 1-9 |

Keybindings can be reconfigured from the "Keys" menu in the client.

## Project Structure

```
RustBall/
├── client/          # Bevy graphical client
│   └── src/
│       ├── main.rs
│       └── keybindings.rs
├── server/          # Authoritative server
│   └── src/
│       ├── main.rs
│       ├── engine.rs    # Physics and game logic
│       ├── network.rs   # WebRTC/Matchbox
│       ├── map/         # Map loading
│       └── input/       # Input handling
├── shared/          # Shared code
│   └── src/
│       ├── lib.rs
│       ├── protocol.rs  # Network messages
│       ├── map.rs       # Map structures
│       └── movements.rs # Animations
├── maps/            # Custom maps
└── images/          # Assets
```

## Maps

The server supports maps in HaxBall format (`.hbs`) and JSON/JSON5. Maps are loaded with `--map`:

```bash
cargo run -p server -- --map maps/futsal_fah.hbs
```

To create compatible maps, you can use the HaxBall editor or create them manually in JSON5.

## Future Development

This project is under active development. Some ideas for contribution:

- Goal system and scoreboard
- Team selection (red/blue)
- In-game chat
- Replay/match recording
- WebAssembly compilation for browser play
- Room/lobby system
- Power-ups and alternative game modes
- Netcode improvements (client-side prediction, reconciliation)
- Support for more map formats
- Integrated map editor

## Contributing

Contributions are welcome. Fork the repo, create a branch, and open a PR.

## License

MIT
