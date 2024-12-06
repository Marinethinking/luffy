FROM rust:1.83-slim-bullseye

# Install debian packaging tools and build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    debhelper \
    cmake \
    pkg-config \
    git \
    # if you use buntu 22 or later, please change to openssl 1.3
    libssl1.1:arm64 \  
    libssl-dev:arm64=1.1.1* \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-deb

# Clone mavlink message definitions if needed
# RUN git clone https://github.com/mavlink/mavlink.git /opt/mavlink

WORKDIR /build

# We'll mount the source code here when running
VOLUME ["/build"]

ENV MAVLINK_DIALECT=ardupilotmega
ENV MAVLINK_PATH=/opt/mavlink/message_definitions/v1.0