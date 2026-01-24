FROM ubuntu:24.04

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copiar binarios
COPY kinetic_ball_server-x86_64-unknown-linux-gnu /usr/local/bin/kinetic_ball_server
COPY matchbox_server /usr/local/bin/matchbox_server
RUN chmod +x /usr/local/bin/kinetic_ball_server /usr/local/bin/matchbox_server

EXPOSE 3537

CMD /usr/local/bin/matchbox_server & /usr/local/bin/kinetic_ball_server --port 3537 --matchbox-url ws://127.0.0.1:3536
