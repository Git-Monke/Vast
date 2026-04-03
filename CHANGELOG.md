# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- `MaterialKind` (Iron / Helium) lives in [`universe`](universe/src/resources.rs) with optional `SpacetimeType` via the `universe/spacetimedb` feature. [`Material`](universe/src/resources.rs) also derives `SpacetimeType` when that feature is enabled.
- **Semantics:** procedural `Material` uses `f64` as spawn **richness**; SpacetimeDB `building.warehouse_inventory` uses `Material` with `f64` as **stored quantity** (units). `building.mining_material` is `Option<Material>` (species + placeholder `f64` for future rates).
- Removed `InventoryEntry`; warehouse stock is `Vec<Material>` only.

### Added

- SpacetimeDB `empire` table: `identity` (primary key), unique `name`, and `credits` (starting balance on registration).
- `register_empire` reducer to create an empire once per identity with validated name and starting credits.
- Root client (`src/main.rs`) subscribes to `empire`, registers via `EMPIRE_NAME` (default `Test Empire`), and logs new empire rows.

### Removed

- Demo `person` table and `add` / `say_hello` reducers from the SpacetimeDB module.
