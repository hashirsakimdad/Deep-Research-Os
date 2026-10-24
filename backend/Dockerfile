# Deep Research OS backend: multi-stage build -> small runtime image.
# Build context is this directory (backend/):  docker build -t deep-research-os-backend backend

# ---- build ----------------------------------------------------------------
FROM rust:1-slim-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential cmake pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Compile dependencies first with stub sources so this layer is cached until Cargo.toml/Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && : > src/lib.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/deep_research_os_backend* target/release/.fingerprint/deep-research-os-backend*

# Real sources. Migrations are embedded in the binary at compile time (sqlx::migrate!).
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release

# ---- runtime --------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home app
COPY --from=build /app/target/release/deep-research-os-backend /usr/local/bin/deep-research-os-backend
USER app

ENV RUST_LOG=info,deep_research_os_backend=info \
    PORT=8080
EXPOSE 8080

# Required at runtime (set them in the host's dashboard, never in the image):
#   DATABASE_URL  GROK_API_KEY  SEARCH_API_KEY  API_ACCESS_KEY  CORS_ORIGIN
# The server listens on $PORT (platforms set it) and exposes /health for health checks.
# SIGTERM from the platform triggers a graceful shutdown that cancels in-flight research.
CMD ["deep-research-os-backend"]
