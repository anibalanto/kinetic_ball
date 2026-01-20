# Migración a WebRTC con Matchbox

Este proyecto ha sido migrado de TCP/Tokio a WebRTC usando `matchbox_socket`.

## ✅ Cambios Realizados

### 1. **Dependencias Actualizadas**
- ✅ Eliminado: Dependencia directa de `tokio` para networking
- ✅ Agregado: `matchbox_socket` v0.10 (WebRTC para Bevy)
- ⚠️ Mantenido: `tokio` v1.40 (requerido internamente por matchbox)

### 2. **Protocolo de Mensajes**
- ✅ Creado `ControlMessage` enum (canal reliable para JOIN, WELCOME, READY, etc.)
- ✅ Creado `GameDataMessage` enum (canal unreliable para Input, GameState, Ping/Pong)
- ✅ Separación de canales: Reliable vs Unreliable

### 3. **Arquitectura**
```
┌─────────────────────┐
│ Matchbox Server     │  Puerto 3536 (señalización WebSocket)
│ (Proceso separado)  │
└──────────┬──────────┘
           │
    ┌──────┴──────┐
    │             │
┌───▼────┐    ┌───▼────┐
│ Server │◄──►│ Client │  WebRTC Data Channels (P2P después de señalización)
│ (Bevy) │    │ (Bevy) │  - Canal 0: Control (reliable)
└────────┘    └────────┘  - Canal 1: GameData (unreliable)
```

### 4. **Código Modificado**
- ✅ `server/src/main.rs`: Reemplazado TCP listener por `WebRtcSocket`
- ✅ `client/src/main.rs`: Reemplazado TCP stream por `WebRtcSocket`
- ✅ `shared/src/protocol.rs`: Agregados `ControlMessage` y `GameDataMessage`

### 5. **Canal Bidireccional Implementado**

El servidor ahora usa un sistema de canales bidireccional para comunicarse entre Bevy y el thread de red:

```rust
// Desde Bevy → Thread de red
enum OutgoingMessage {
    ToOne { peer_id, channel, data },  // Enviar a un cliente específico
    Broadcast { channel, data },        // Enviar a todos los clientes
}

// Flujo:
// 1. Sistema de Bevy serializa el mensaje
// 2. Lo envía via NetworkSender (mpsc::Sender<OutgoingMessage>)
// 3. Thread de red lo recibe y lo transmite via WebRTC socket
```

**Ejemplo - Enviar Welcome**:
```rust
// En process_network_messages (sistema de Bevy)
let welcome_msg = ControlMessage::Welcome { ... };
let data = bincode::serialize(&welcome_msg)?;
network_tx.send(OutgoingMessage::ToOne {
    peer_id,
    channel: 0,  // Canal reliable
    data,
})?;
```

**Ejemplo - Broadcast GameState**:
```rust
// En broadcast_game_state (sistema de Bevy)
let game_data_msg = GameDataMessage::GameState { ... };
let data = bincode::serialize(&game_data_msg)?;

// Enviar a cada jugador ready
for player in players.iter().filter(|p| p.is_ready) {
    network_tx.send(OutgoingMessage::ToOne {
        peer_id: player.peer_id,
        channel: 1,  // Canal unreliable
        data: data.clone(),
    })?;
}
```

## 🚀 Cómo Ejecutar

### Prerequisitos

1. **Instalar matchbox_server** (servidor de señalización):
   ```bash
   cargo install matchbox_server
   ```

### Ejecución

**Paso 1: Iniciar servidor de señalización**
```bash
matchbox_server --port 3536
```
Verás: `Matchbox listening on 0.0.0.0:3536`

**Paso 2: Iniciar servidor de juego**
```bash
cargo run --bin server
```
El servidor se conectará a `ws://127.0.0.1:3536/game_server`

**Paso 3: Iniciar clientes**
```bash
cargo run --bin client -- --name Player1
cargo run --bin client -- --name Player2
```

Los clientes se conectarán al mismo `ws://127.0.0.1:3536/game_server` y establecerán conexiones WebRTC P2P con el servidor.

## 🔧 Configuración Avanzada

### Cambiar puerto de señalización

**Servidor**:
```bash
cargo run --bin server -- --signaling-port 8080
```

**Cliente**:
```bash
cargo run --bin client -- --server ws://127.0.0.1:8080
```

## ⚠️ Limitaciones Actuales

### 1. **NAT Traversal**
**Estado**: ⚠️ Solo funciona en LAN

matchbox_server por defecto NO tiene STUN/TURN servers configurados, por lo que las conexiones WebRTC solo funcionan en LAN o localhost.

**Solución para producción**:
- Configurar STUN server público (Google, Cloudflare)
- Configurar TURN server si hay NATs estrictos

## 📋 Estado de Implementación

- [x] **Canal bidireccional Bevy ↔ Thread de red** ✅ COMPLETADO
- [x] **Broadcast de GameState** ✅ COMPLETADO (60Hz via canal unreliable)
- [x] **Envío de Welcome** ✅ COMPLETADO (después de JOIN via canal reliable)
- [x] **Handshake completo** ✅ COMPLETADO (JOIN → WELCOME → READY)
- [ ] Agregar configuración de STUN servers para NAT traversal
- [ ] Testing con múltiples clientes (4+ jugadores)
- [ ] Medir latencia WebRTC vs TCP original
- [ ] Manejo de reconexión automática

## 🎯 Ventajas de WebRTC

1. **Menor latencia**: UDP por debajo en lugar de TCP
2. **P2P real**: Conexiones directas después de señalización
3. **Canales mixtos**: Reliable + Unreliable simultáneos
4. **Estandarizado**: Funciona en browsers también (futuro cliente web)
5. **NAT traversal**: STUN/TURN para redes complejas

## 🐛 Debugging

### Servidor no se conecta a matchbox
- Verificar que matchbox_server esté corriendo: `ps aux | grep matchbox_server`
- Verificar puerto: `lsof -i :3536`

### Cliente no recibe WELCOME
- Verificar logs del servidor: debe mostrar "🎮 Player X joined"
- Verificar que el código de broadcast esté implementado (actualmente comentado)

### Pérdida de paquetes extrema
- El canal unreliable puede perder hasta 5% de paquetes, es normal
- Si > 10%, verificar red local

## 📚 Referencias

- [matchbox_socket docs](https://github.com/johanhelsing/matchbox)
- [WebRTC Bevy example](https://github.com/johanhelsing/matchbox/tree/main/examples)
- [Plan de migración](/home/anibal/.claude/plans/sparkling-sniffing-castle.md)
