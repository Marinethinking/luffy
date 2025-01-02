#!/bin/bash
set -e

# Default components list
COMPONENTS="luffy-gateway luffy-launcher luffy-media"

# If a component is specified as an argument, only build that one
if [ $# -eq 1 ]; then
    if [[ $COMPONENTS =~ $1 ]]; then
        COMPONENTS=$1
    else
        echo "Error: Invalid component '$1'"
        echo "Valid components are: $COMPONENTS"
        exit 1
    fi
fi

# Build the builder image
docker build -t luffy-builder -f docker/build-deb.Dockerfile .

# Build debs for each component
for component in $COMPONENTS; do
    echo "Building $component..."
    docker run --rm \
        -v "$(pwd):/build" \
        -v "$(pwd)/../:/workspace" \
        --platform linux/arm64 \
        luffy-builder \
        bash -c "echo 'Checking config files...' && \
                 ls -la /workspace/luffy-deploy/config/ && \
                 echo 'Building...' && \
                 cd /workspace/$component && \
                 cargo deb --target aarch64-unknown-linux-gnu"
done

# Collect the built packages
mkdir -p dist
for component in $COMPONENTS; do
    echo "Copying $component deb package..."
    if [ -f "../target/aarch64-unknown-linux-gnu/debian/"*"$component"*.deb ]; then
        cp "../target/aarch64-unknown-linux-gnu/debian/"*"$component"*.deb dist/
    else
        echo "Warning: No .deb file found for $component"
        echo "Expected path: ../target/aarch64-unknown-linux-gnu/debian/*${component}*.deb"
        ls -la "../target/aarch64-unknown-linux-gnu/debian/" || echo "Directory does not exist"
    fi
done

echo "Built packages are in dist/"
