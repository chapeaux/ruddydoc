# Multi-stage build for RuddyDoc
# Stage 1: Build the Rust binary
FROM rust:1.85-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY ontology ./ontology

# Build release binary
RUN cargo build --release -p ruddydoc-cli

# Strip the binary to reduce size
RUN strip target/release/ruddydoc

# Stage 2: Create minimal runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /build/target/release/ruddydoc /usr/local/bin/ruddydoc

# Set up a non-root user
RUN useradd -m -u 1000 ruddydoc
USER ruddydoc
WORKDIR /home/ruddydoc

ENTRYPOINT ["ruddydoc"]
CMD ["--help"]
