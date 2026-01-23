FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

# Copiar binarios
COPY kinetic_ball_server matchbox_server /usr/local/bin/
RUN chmod +x /usr/local/bin/kinetic_ball_server /usr/local/bin/matchbox_server

EXPOSE 3537

# Usar rutas absolutas y asegurar que el contenedor no cierre el shell
CMD /usr/local/bin/matchbox_server & /usr/local/bin/kinetic_ball_server --port 3537 --matchbox-url ws://127.0.0.1:3536
