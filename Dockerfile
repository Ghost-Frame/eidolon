FROM rust:1-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY eidolon-lib/ eidolon-lib/
COPY eidolon-daemon/ eidolon-daemon/
COPY eidolon-cli/ eidolon-cli/
COPY eidolon/ eidolon/
COPY eidolon-tui/ eidolon-tui/

RUN cargo build --release --bin eidolon-daemon

FROM debian:bookworm-slim

LABEL org.opencontainers.image.title="Eidolon" \
      org.opencontainers.image.description="Neural brain for AI agents" \
      org.opencontainers.image.url="https://codeberg.org/GhostFrame/eidolon" \
      org.opencontainers.image.source="https://codeberg.org/GhostFrame/eidolon" \
      org.opencontainers.image.licenses="Elastic-2.0" \
      org.opencontainers.image.vendor="Syntheos"

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/eidolon-daemon /usr/local/bin/eidolon-daemon

RUN mkdir -p /app/data
WORKDIR /app
VOLUME /app/data

EXPOSE 7700

ENV EIDOLON_PORT=7700
ENV EIDOLON_HOST=0.0.0.0

CMD ["eidolon-daemon"]
