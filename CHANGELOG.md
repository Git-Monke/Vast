# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- SpacetimeDB `building`: optional **`owner`** (`Option<Identity>`) and **`attack_mode`** (`Option<ShipAttackMode>`) for **military garrisons** only; other building kinds use `None`. Index `building_by_garrison_owner` on `owner`. Reducers do not enforce garrison invariants yet.
- [`ShipStats`](universe/src/ships.rs): `speed_tenths_ly_s` (`u32`) replaced by **`speed_lys` (`f64`)** — speed in light-years per second directly. Removed `speed_ly_per_s`; [`travel_duration_secs`](universe/src/ships.rs) now takes `speed_lys`.
- `MaterialKind` (Iron / Helium) lives in [`universe`](universe/src/resources.rs) with optional `SpacetimeType` via the `universe/spacetimedb` feature. [`Material`](universe/src/resources.rs) also derives `SpacetimeType` when that feature is enabled.
- **Semantics:** procedural `Material` uses `f64` as spawn **richness**; SpacetimeDB `building.warehouse_inventory` uses `Material` with `f64` as **stored quantity** (units). `building.mining_material` is `Option<Material>` (species + placeholder `f64` for future rates).
- Removed `InventoryEntry`; warehouse stock is `Vec<Material>` only.

### Added

- SpacetimeDB `empire` table: `identity` (primary key), unique `name`, and `credits` (starting balance on registration).
- `register_empire` reducer to create an empire once per identity with validated name and starting credits.
- Root client (`src/main.rs`) subscribes to `empire`, registers via `EMPIRE_NAME` (default `Test Empire`), and logs new empire rows.
- SpacetimeDB `ship` table: `owner`, [`ShipStats`](universe/src/ships.rs), [`ShipAttackMode`](universe/src/ships.rs), [`ShipLocation`](universe/src/ships.rs) (`AtPlanet` / `InTransit` with timestamps). Index `ship_by_owner`. No spawn/warp reducers yet.
- [`travel_duration_secs`](universe/src/ships.rs): `duration = distance_ly / speed_lys` using [`ShipStats::speed_lys`](universe/src/ships.rs) (light-years per second, `f64`).

### Removed

- Demo `person` table and `add` / `say_hello` reducers from the SpacetimeDB module.
