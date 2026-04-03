#!/bin/bash
set -e
cargo build --package universe
cargo build --package explorer 
cd ./spacetimedb && spacetime build
