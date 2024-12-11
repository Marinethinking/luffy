# Luffy

A smart vehicle onboard program that provides network connectivity and web-based monitoring/control interface.

## Features

- **MQTT Broker**: Local broker for onboard communication
- **Web Server**: Web interface for monitoring and control
- **Telemetry**: Data publishing to both local and remote MQTT brokers
- **Command**: Command subscription from upstream node to vehicle
- **OTA**: Over-the-air update for the vehicle

```
luffy/
├── luffy-launcher/       # Main launcher/orchestrator
├── luffy-gateway/       # Communication services
├── luffy-video/         # Video processing service
├── luffy-common/        # Shared libraries
└── luffy-deploy/        # Deployment configurations
```

## Installation

Follow [luffy-deploy/README.md](luffy-deploy/README.md)

## Development Setup

### Prerequisites
- Rust toolchain (via [rustup](https://rustup.rs/))
- AWS credentials configured locally
- SITL simulator (for testing)
- VSCode with rust-analyzer extension (recommended)
- dev tools: 
  - `cargo install cargo-edit`   
  - `cargo install cargo-watch`  -> `cargo watch -d 2000 -x run`



### VSCode Setup
Add to settings.json (Cmd/Ctrl + ,):
```json
{
    "rust-analyzer.cargo.buildBeforeRun": true
}
```

### Running Locally
1. Start SITL simulator:
   ```bash
   sim_vehicle.py --out <YOUR_IP>:<PORT>
   ```

2. Build and run:
   ```bash
   cargo build
   cargo run
   ```

3. Access web interface at [http://localhost:9000](http://localhost:9000)

## Release 

### Release Process
1. Change version to a.b.c in `Cargo.toml`
2. Check release config [OTA] in `config/dev.toml`
3. Commit and push changes to github
4. git tag va.b.c
5. git push origin va.b.c

If release CI failed, change ci script and re-release:
   ```bash
   git tag -d v0.2.2  # delete old tag locally
   git push origin :refs/tags/v0.2.2  # delete old tag remotely
   git tag v0.2.2
   git push origin v0.2.2
   ```

## Resources
- [AWS SDK for Rust Documentation](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html)
- [AWS SDK Examples](https://github.com/awsdocs/aws-doc-sdk-examples/tree/main/rustv1) 


