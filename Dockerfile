FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY kinetic_ball_server matchbox_server /usr/local/bin/

EXPOSE 3537

CMD matchbox_server & kinetic_ball_server --port 3537 --matchbox-url ws://127.0.0.1:3536
