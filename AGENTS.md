# Vast Repo Rules

## Structure
Cargo workspace excludes `spacetimedb/` (SpacetimeDB module, edition 2024).

| Crate | Purpose |
|-------|---------|
| spacetimedb/ | Server module (test: `cargo test --manifest-path spacetimedb/Cargo.toml`) |
| universe/ | Procedural gen lib (stars, planets, resources; `spacetimedb` feature for types) |
| vast-bindings/ | **Generated** client bindings (**NEVER edit** `src/mod.rs`) |
| explorer/ | egui galaxy explorer (uses `frame_tick()` in loop) |
| src/ | Root client (uses `run_threaded()`) |

## Commands
```bash
spacetime start                    # Local server (:3000), DB 'vast'
make publish                       # Publish module to local DB
spacetime build                    # Build WASM
spacetime generate --lang rust --out-dir vast-bindings/src --module-path spacetimedb  # Bindings after backend changes
cargo run -p explorer --release    # Egui UI
cargo run                          # Root client
./test.sh                          # Build crates + module + spacetimedb tests
make reset-db                      # Restart server + publish (clears data)
spacetime logs vast                # Logs
```

## Workflow (for testing changes)
3. `cargo check`
4. `./test.sh`

## Notes
- Clients: `vast-bindings::*`, defaults `SPACETIMEDB_HOST=localhost:3000` `DB_NAME=vast`
- Universe: deterministic `(x,y)` → stars; coords `i32` (0.1 ly/step), 500k ly radius disk
- See `DESIGN.md` (game), `CHANGELOG.md` (state), `CLAUDE.md` (overview)

---

# SpacetimeDB Rust SDK
## ⛔ COMMON MISTAKES (preserve all existing SpacetimeDB content verbatim)
