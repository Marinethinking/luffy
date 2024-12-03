FROM rust:1.76-slim as builder
WORKDIR /usr/src/luffy
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/luffy/target/release/luffy /usr/local/bin/
COPY config /etc/luffy/config
EXPOSE 9000
CMD ["luffy"] 