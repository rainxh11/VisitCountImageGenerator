# Visits Count Image Generator

Rust service using Axum and SQLite. The original PNG and Caskaydia font are embedded in the executable. Each generated image records one visit and displays that username's total, with cyan digits, gray padding, and rounded transparent corners. No application, access, or database logging is configured; startup failures exit with a nonzero status without printing errors.

## Run locally

Install Rust 1.99.1 or later, then run:

```sh
cargo run --release --locked
```

Configuration uses environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `BIND_ADDRESS` | `0.0.0.0:8000` | HTTP listening address |
| `DATABASE_PATH` | `data/visits.sqlite3` | SQLite file; parent directories are created automatically |

## Docker

```sh
docker compose up --build -d
```

The multi-stage Dockerfile uses cargo-chef prepare/cook stages to cache dependencies and runs the executable as an unprivileged user. Compose exposes port 8000 to the Docker network; add a port mapping such as `8000:8000` when direct host access is needed. The `visit_count_db` volume persists SQLite and its WAL files under `/data`.

SQLite uses `/data/visits.sqlite3`, separate from the old LiteDB `/data/database.db`. Existing LiteDB records are not imported automatically. Keep the old file if historical data is needed.

## HTTP API

All responses include `Cache-Control: max-age=0, no-cache, no-store, must-revalidate`.

| Method and route | Result |
| --- | --- |
| `GET /visit-count/generate/{username}.png` | Records one visit and returns `image/png` with the updated per-user count |
| `GET /visit-count/query/{username}` | JSON visit array, newest first; unknown users return `[]` |
| `GET /visit-count/query` | JSON object containing `stats` and `grouped`, keyed by username |

Visit JSON has `username`, `request`, `id`, and `createdOn` (UTC RFC 3339). Request metadata retains `Cookies` (Key/Value objects), `Headers` (strings), `Host` (string), `Query` (distinct query parameter names), and `Path` (string). Headers are normalized to lowercase by the HTTP stack. Query responses still contain request metadata, as in the original service; disabling logging does not disable visit storage.

`stats[username]` contains `count` and `visitors` (the first X-Forwarded-For header string for each visit, when present). Counts are per user, correcting the original aggregate endpoint's global-count bug. Forwarded headers are stored as supplied. Invalid image suffixes and empty usernames return 404; storage/rendering failures return an empty 500 response. Insert and count run in one transaction so concurrent requests cannot lose increments. JSON uses camelCase for response properties, as in ASP.NET's web defaults.

The existing image layout is retained, but Rust font rendering can produce small pixel differences from ImageSharp.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Integration tests exercise PNG decoding, changing counters, metadata, grouped statistics, persistence, cache headers, invalid paths, and concurrent increments.
