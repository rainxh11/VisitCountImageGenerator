FROM rust:1.99.1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY Caskaydia.otf VisitCount.png ./
RUN cargo build --release --locked --bin visit-count-generator

FROM debian:bookworm-slim AS runtime
RUN groupadd --gid 10001 app && useradd --uid 10001 --gid app --no-create-home app \
    && mkdir /data && chown app:app /data
WORKDIR /app
COPY --from=builder /app/target/release/visit-count-generator /app/visit-count-generator
ENV BIND_ADDRESS=0.0.0.0:8000 DATABASE_PATH=/data/visits.sqlite3
USER app
EXPOSE 8000
ENTRYPOINT ["/app/visit-count-generator"]
