FROM rust:1.83-slim as builder
WORKDIR /usr/src/luffy

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    cmake \
    python3 \
    python3-pip \
    && pip3 install pymavlink \
    && rm -rf /var/lib/apt/lists/*

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
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/luffy/target/release/luffy /usr/local/bin/
COPY config /etc/luffy/config
EXPOSE 9000
CMD ["luffy"] 