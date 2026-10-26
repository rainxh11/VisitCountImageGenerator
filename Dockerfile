# syntax=docker/dockerfile:1
FROM rust:1.99.1-alpine AS chef
RUN apk add --no-cache build-base \
    && cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY Caskaydia.otf VisitCount.png ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked --bin visit-count-generator
RUN mkdir -p /runtime/data /runtime/tmp && chmod 1777 /runtime/tmp

FROM scratch AS runtime
WORKDIR /app
COPY --from=builder --chown=10001:10001 /runtime/ /
COPY --from=builder /app/target/release/visit-count-generator /app/visit-count-generator
ENV BIND_ADDRESS=0.0.0.0:8000 DATABASE_PATH=/data/visits.sqlite3
USER 10001:10001
EXPOSE 8000
ENTRYPOINT ["/app/visit-count-generator"]
