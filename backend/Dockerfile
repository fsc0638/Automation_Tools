FROM rust:1.82-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev libpq-dev libssh2-1-dev libgit2-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -f target/release/deps/kway*
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 libpq5 libssh2-1 libgit2-1.5 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/kway-dev-backend .
COPY --from=builder /app/migrations ./migrations
EXPOSE 8080
CMD ["./kway-dev-backend"]
