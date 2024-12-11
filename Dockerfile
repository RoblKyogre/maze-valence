FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release
FROM debian:stable-slim AS final
COPY --from=builder /app/target/release/maze-valence /maze-valence
COPY maze.toml /data/maze.toml
CMD /maze-valence /data/maze.toml
