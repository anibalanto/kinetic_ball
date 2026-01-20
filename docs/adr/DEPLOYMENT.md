# 🌐 Guía de Deployment para Internet

Esta guía explica cómo hacer que RustBall funcione en internet con jugadores remotos.

## Arquitectura en Internet

```
Internet
   │
   ├─ Servidor Matchbox (Señalización WebRTC)
   │  - Público, accesible en internet
   │  - Puerto: 3536 (o el que elijas)
   │  - Ejemplo: ws://tu-servidor.com:3536
   │
   ├─ Servidores STUN (NAT Traversal)
   │  - Google: stun.l.google.com:19302
   │  - Cloudflare: stun.cloudflare.com:3478
   │  ✅ YA CONFIGURADO EN EL CÓDIGO
   │
   └─ Conexiones WebRTC P2P
      - Servidor de juego ←→ Clientes
      - Datos directos después de señalización
```

## 🚀 Opción 1: Deploy en VPS/Cloud (Recomendado)

### Paso 1: Alquilar un VPS

Opciones económicas:
- **DigitalOcean**: $6/mes (Droplet)
- **Linode**: $5/mes
- **Vultr**: $6/mes
- **Oracle Cloud**: Free tier (gratis)
- **Google Cloud**: $10 crédito gratis

### Paso 2: Configurar el VPS

```bash
# SSH al servidor
ssh root@tu-servidor-ip

# Instalar Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Instalar matchbox_server
cargo install matchbox_server

# Abrir puertos en firewall
ufw allow 3536/tcp  # Matchbox signaling (WebSocket)
ufw allow 9000/udp  # WebRTC data (opcional, P2P puede usar cualquier puerto)
```

### Paso 3: Ejecutar matchbox_server como servicio

```bash
# Crear archivo de servicio systemd
sudo nano /etc/systemd/system/matchbox.service
```

Contenido del archivo:
```ini
[Unit]
Description=Matchbox WebRTC Signaling Server
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/root
ExecStart=/root/.cargo/bin/matchbox_server --port 3536 --host 0.0.0.0
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
# Habilitar e iniciar el servicio
sudo systemctl daemon-reload
sudo systemctl enable matchbox
sudo systemctl start matchbox
sudo systemctl status matchbox
```

### Paso 4: Ejecutar el Servidor de Juego

```bash
# Clonar el repositorio en el servidor
git clone https://github.com/tu-usuario/RustBall.git
cd RustBall

# Compilar en modo release
cargo build --release --bin server

# Ejecutar el servidor
./target/release/server --signaling-port 3536
```

### Paso 5: Conectar Clientes desde Casa

En tu computadora local:

```bash
# Conectar usando la IP pública o dominio del VPS
cargo run --bin client -- --server ws://TU_IP_PUBLICA:3536 --name "TuNombre"
```

Ejemplo:
```bash
cargo run --bin client -- --server ws://192.168.1.100:3536 --name "Player1"
```

---

## 🏠 Opción 2: Port Forwarding desde Casa

Si tienes acceso al router:

### Paso 1: Configurar Port Forwarding en el Router

1. Accede a tu router (usualmente 192.168.1.1 o 192.168.0.1)
2. Busca "Port Forwarding" o "NAT"
3. Crear regla:
   - **Puerto externo**: 3536
   - **Puerto interno**: 3536
   - **IP interna**: Tu PC (ejemplo: 192.168.1.100)
   - **Protocolo**: TCP

### Paso 2: Obtener tu IP Pública

```bash
curl ifconfig.me
```

### Paso 3: Ejecutar Matchbox y Servidor

```bash
# Terminal 1: Matchbox
matchbox_server --port 3536 --host 0.0.0.0

# Terminal 2: Servidor de juego
cargo run --bin server -- --signaling-port 3536
```

### Paso 4: Compartir URL con Amigos

```bash
# Tus amigos deben conectarse con:
cargo run --bin client -- --server ws://TU_IP_PUBLICA:3536 --name "Amigo1"
```

**⚠️ Limitaciones**:
- Tu IP pública puede cambiar (usar DynamicDNS como No-IP)
- Algunos ISPs bloquean puertos
- Requiere dejar la PC encendida

---

## ☁️ Opción 3: Servicio Gratuito Matchbox

Usar un servidor matchbox público ya existente:

```bash
# Usar servidor público de matchbox (si existe)
cargo run --bin server -- --server wss://match.johanhelsing.studio
cargo run --bin client -- --server wss://match.johanhelsing.studio --name "Player1"
```

**Nota**: Verificar disponibilidad en https://github.com/johanhelsing/matchbox

---

## 🐳 Opción 4: Docker en Cloud Run / Fly.io (Avanzado)

### Dockerfile para Matchbox + Servidor

Crear `Dockerfile`:
```dockerfile
FROM rust:1.75 as builder

# Instalar matchbox_server
RUN cargo install matchbox_server

# Copiar código
WORKDIR /app
COPY . .
RUN cargo build --release --bin server

FROM debian:bookworm-slim
COPY --from=builder /usr/local/cargo/bin/matchbox_server /usr/local/bin/
COPY --from=builder /app/target/release/server /usr/local/bin/rustball-server

# Exponer puertos
EXPOSE 3536 9000

# Script de inicio
CMD matchbox_server --port 3536 --host 0.0.0.0 & rustball-server --signaling-port 3536
```

Deploy en Fly.io:
```bash
fly launch
fly deploy
```

---

## 🧪 Testing de Conexión

### Verificar que Matchbox está accesible

```bash
# Desde otra computadora
curl http://TU_IP:3536/health
# Debería retornar 200 OK
```

### Testing con WebSocket

```bash
# Instalar websocat
cargo install websocat

# Probar conexión WebSocket
websocat ws://TU_IP:3536/game_server
```

---

## 🔧 Troubleshooting

### Error: "Connection refused"
- ✅ Verificar que matchbox_server está corriendo: `ps aux | grep matchbox`
- ✅ Verificar firewall: `sudo ufw status`
- ✅ Verificar puerto correcto: `lsof -i :3536`

### Error: "WebRTC connection failed"
- ✅ STUN servers ya están configurados en el código ✅
- ✅ Verificar que ambos peers pueden alcanzar los STUN servers
- ⚠️ Si falla, podría ser NAT simétrico (necesitas servidor TURN)

### Alta latencia
- Usar VPS cercano geográficamente
- Optimizar tick rate si es necesario

### No se conecta después de señalización
- Verificar logs del servidor: `journalctl -u matchbox -f`
- Algunos firewalls corporativos bloquean WebRTC (probar desde red móvil)

---

## 📊 Costos Estimados

### Setup Mínimo (1-4 jugadores)
- **VPS pequeño**: $5-10/mes
- **Dominio** (opcional): $12/año
- **Total**: ~$5/mes

### Setup con TURN (NATs muy restrictivos)
- **VPS + TURN server**: $10-20/mes

### Alternativa Gratis
- **Oracle Cloud Free Tier**: Gratis permanentemente
- **Ngrok túnel temporal**: Gratis (con límites)

---

## 🎯 Recomendación Final

**Para empezar rápido (testing con amigos)**:
1. Port forwarding en tu router
2. Compartir tu IP pública
3. STUN servers ya configurados ✅

**Para producción (juego público)**:
1. VPS en DigitalOcean ($6/mes)
2. Matchbox como servicio systemd
3. Dominio opcional para URL fácil
4. STUN servers ya configurados ✅

**STUN ya incluido**: No necesitas configurar nada más, el código ya tiene los servidores STUN de Google y Cloudflare integrados. Solo necesitas el servidor matchbox accesible desde internet.
