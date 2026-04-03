# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VAST is a space simulation game built with SpacetimeDB (Rust backend) and a Rust client. The universe uses procedural generation via hash functions — the same coordinates always produce the same star system, deterministically, with no stored data.

## Commands

```bash
# Build the SpacetimeDB backend module (compiles to WASM)
spacetime build

# Publish backend to local SpacetimeDB server
make publish
# or equivalently:
spacetime publish --server http://localhost:3000 --anonymous vast

# Run the Rust client
cargo run

# View server logs
spacetime logs vast

# Regenerate client bindings after backend changes
spacetime generate --lang rust --out-dir src/module_bindings --module-path spacetimedb

# Start local SpacetimeDB server
spacetime start

# Interact via CLI (examples)
spacetime call add "Alice"
spacetime sql "SELECT * FROM person"
```

## Repository Structure

```
vast/
├── spacetimedb/src/lib.rs   # Backend: SpacetimeDB tables and reducers (compiled to WASM)
├── src/main.rs              # Rust client: connects, subscribes, calls reducers
├── src/module_bindings/     # Auto-generated — DO NOT edit, regenerate with spacetime generate
├── universe/src/            # Procedural generation library (pure Rust, no SpacetimeDB dep)
│   ├── hasher.rs            # point_hash(x, y, seed) → u64, point_to_random() → f64
│   ├── checker.rs           # star_is_at_point(x, y) — density function for star placement
│   └── generator.rs        # StarSystem, Planet, StarType, PlanetType — generation logic
└── Makefile                 # build / publish shortcuts
```

## Architecture

**Three components:**

1. **Backend** (`spacetimedb/`) — Compiled to WASM, runs inside SpacetimeDB. Defines tables and reducers. Currently has a minimal `person` table as a scaffold; the real game data model is not yet implemented.

2. **Client** (`src/`) — Rust binary using `spacetimedb-sdk`. Connects via env vars `SPACETIMEDB_HOST` (default `http://localhost:3000`) and `SPACETIMEDB_DB_NAME` (default `my-db`). Subscribes to tables and registers callbacks.

3. **Universe library** (`universe/`) — Pure procedural generation, no network or SpacetimeDB dependency. Produces deterministic star systems from `(x, y)` coordinates using `point_hash`. This library is intended to be shared between server and client for client-side prediction.

**Procedural generation design:** Star density follows an exponential falloff from the origin (`min_spacing = 0.3 ly`, `growth_rate = 0.0001535`). Each star is typed (Red → NeutronStar) and has planets, all derived from a single 64-bit hash of its coordinates.

## Current State

`universe/src/generator.rs` has several bugs and is not compiling:
- Uses undefined macro `threshold!` (defined as `thresh!`)
- Uses variable `hash` instead of `star_hash` in `hash_to_planet_count` and `hash_to_star_type`
- `StarType::from_index` returns `Some(...)` but the function signature is `-> Self` (not `Option<Self>`)
- `generate_star` is incomplete — doesn't build or return a `StarSystem`
- Missing semicolon after `use` statement on line 1

The backend (`spacetimedb/lib.rs`) is currently a placeholder (ChatApp scaffold) and does not yet expose any universe/game data.

## SpacetimeDB Rules

See the system-provided rules (loaded from `spacetimedb-rust.mdc`) for complete SpacetimeDB Rust API patterns — table macros, reducer syntax, update patterns, client connection, etc.
