# Luffy

A smart vehicle onboard program that provides network connectivity and web-based monitoring/control interface.

## Features

- **MQTT Broker**: Local broker for onboard communication
- **Web Server**: Web interface for monitoring and control
- **Telemetry**: Data publishing to both local and remote MQTT brokers
- **Command**: Command subscription from upstream node to vehicle

## Development Setup

### Prerequisites
- Rust toolchain (via [rustup](https://rustup.rs/))
- AWS credentials configured locally
- SITL simulator (for testing)
- VSCode with rust-analyzer extension (recommended)
- dev tools: 
  - `cargo install cargo-edit`   
  - `cargo install cargo-watch`  -> `cargo watch -d 2000 -x run`

### Configuration
1. Configure `config/dev.toml`:
   - Set mavlink URL
   - Configure OTA settings (for release):
     - `check_interval`
     - `version_check_url`
     - `s3_bucket`
     - `bin_name`
     - `release_path`

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

### Prerequisites
- Install cross-compilation tool:
   ```bash
   cargo install cross --git https://github.com/cross-rs/cross
   ```
- Test OTA:
  ```bash
  cargo run --bin test_ota
  ```

### Release Process
1. Change version to a.b.c in `Cargo.toml`
2. Check release config [OTA] in `config/dev.toml`
3. Commit and push changes to github
4. git tag va.b.c
5. git push origin va.b.c



## Resources
- [AWS SDK for Rust Documentation](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html)
- [AWS SDK Examples](https://github.com/awsdocs/aws-doc-sdk-examples/tree/main/rustv1) 


## Installation
- Prerequisite:
  - AWS credentials on target machine, with minimum required permission:
    - `s3:GetObject`
    - `s3:PutObject`
    - `lambda:InvokeFunction`  
