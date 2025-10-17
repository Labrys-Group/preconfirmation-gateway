# ============================================================================
# Stage 1: Builder
# ============================================================================
FROM rust:1.90-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libpq-dev \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /build

# Copy dependency manifests first for better layer caching
# This allows Docker to cache dependencies if only source code changes
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY migrations ./migrations
COPY rustfmt.toml ./

# Copy the slqx query cache
COPY .sqlx .sqlx

# Build the application in release mode
# --locked ensures Cargo.lock is used exactly as specified
# --release enables optimizations for production
RUN cargo build --release --locked

# ============================================================================
# Stage 2: Runtime
# ============================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
# libpq5: PostgreSQL client library
# ca-certificates: Required for HTTPS connections to external APIs
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user for security
RUN useradd -m -u 1000 -s /bin/bash gateway

# Set working directory
WORKDIR /app

# Copy the compiled binary from builder stage
COPY --from=builder /build/target/release/preconfirmation-gateway ./preconfirmation-gateway

# Copy migrations directory (required for automatic migration on startup)
COPY --from=builder /build/migrations ./migrations

# Copy configuration file
COPY config.toml ./config.toml

# Change ownership to non-root user
RUN chown -R gateway:gateway /app

# Switch to non-root user
USER gateway

# Expose ports
# 8080: JSON-RPC server port
# 9090: Prometheus metrics endpoint
EXPOSE 8080 9090

# Health check (optional but recommended)
# Checks if the metrics endpoint is responding
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9090/metrics || exit 1

# Set entrypoint
ENTRYPOINT ["./preconfirmation-gateway"]

# ============================================================================
# Build instructions:
# docker build -t preconfirmation-gateway:latest .
#
# Run instructions:
# docker run -p 8080:8080 -p 9090:9090 \
#   -e DATABASE_URL="postgresql://user:pass@host:5432/db" \
#   -e BEACON_API_ENDPOINT="https://beacon-api.example.com" \
#   -e COMMITTER_PRIVATE_KEY="your_ecdsa_key" \
#   -e BLS_PRIVATE_KEY="your_bls_key" \
#   preconfirmation-gateway:latest
#
# Required environment variables:
# - DATABASE_URL: PostgreSQL connection string
# - BEACON_API_ENDPOINT: Beacon API endpoint URL
# - COMMITTER_PRIVATE_KEY: ECDSA private key (64 hex chars, no 0x prefix)
# - BLS_PRIVATE_KEY: BLS private key (64 hex chars, no 0x prefix)
#
# Optional environment variables:
# - RUST_LOG: Logging level (default: info)
# ============================================================================
