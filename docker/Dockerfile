FROM rust:1.83-slim as builder
WORKDIR /usr/src/luffy

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    cmake \
    git \
    && rm -rf /var/lib/apt/lists/*

# Get mavlink message definitions
RUN git clone https://github.com/mavlink/mavlink.git /tmp/mavlink

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo fetch
RUN cargo build --release
RUN rm src/main.rs

# Build actual application
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/src/luffy/target/release/luffy /usr/local/bin/
COPY config /etc/luffy
COPY static/ /app/static/
EXPOSE 9000
CMD ["luffy"] 