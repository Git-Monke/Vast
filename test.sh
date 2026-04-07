#!/bin/bash
set -e
cargo build --package universe
cargo build --package explorer 
spacetime build
cargo test --manifest-path spacetimedb/Cargo.toml
