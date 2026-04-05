# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- **SpacetimeDB:** **`place_building`** and **`upgrade_building`** — instant placement on a **planet slot**; **stationed ship** at the star required (`AtStar` only); **hardcoded** min ship size (kt) per **level** (levels 1–12); credits per kind/level in [`spacetimedb/src/building_rules.rs`](spacetimedb/src/building_rules.rs). **SalesDepot:** owned (`owner` set), **no gameplay level** (stored `level` fixed at 1), price **`1000 × 2^n`** credits where **`n`** is how many Sales Depots you already own. **MiningDepot / Warehouse / ShipDepot:** **`owner` is `None`**. **Enemy military garrison** on a planet blocks **build and upgrade** there unless it is yours.
- **Galaxy Explorer:** **Construction** section on the selected star — pick planet, slot, kind, level (except Sales Depot), place building; **upgrade +1** on listed structures; cost preview mirrors server rules ([`explorer/src/building_economy.rs`](explorer/src/building_economy.rs), keep in sync with `building_rules`).

- **Galaxy Explorer:** ships **in transit** show a **cyan bubble** along the route line, interpolated from **`depart_at` / `arrive_at`** vs wall-clock time so you can see progress toward the destination.

- **Galaxy Explorer:** SpacetimeDB session tokens are **per empire name** — e.g. `~/.cache/vast/explorer_tokens/<sanitized_name>.txt`. Enter the **same name** and **Connect** to resume that empire; a **new name** gets a fresh identity (then **Start** to register).

- **[`universe::star_id`](universe/src/star_id.rs):** reversible **star IDs** — [`star_display_id`](universe/src/star_id.rs) / [`parse_star_id`](universe/src/star_id.rs); standard form `LL-rx-ry` (10 000-grid blocks, `A–Z` / `a–z`), fallback `!x,y` for huge coords.
- **Galaxy Explorer:** selected system shows **Star ID** with **Copy** and **Use for warp**; **Warp** section parses ID → `order_warp` for the **selected ship**; toast messages for warp errors; docked ships show **battery charge** time until warp allowed.
- **Galaxy Explorer:** star system side panel includes **Your ships** above the planet list: shows **your** ships at that system (in system or in transit from/to it) with **ID**, full **stats**, **attack mode**, **cargo**, and **Center on map**; click the ship title to select (highlighted frame). Selection clears when you pick another star.
- **Galaxy Explorer:** bootstrap uses **`register_empire_then`** and nested **`spawn_starter_ship_then`** so server **`Err(String)`** messages (name taken, no empire, no starter slot, etc.) show in the welcome UI; results are passed through an **`mpsc` channel** and merged into **`bootstrap_error`**, with **`request_repaint`** when messages arrive.
- **Galaxy Explorer:** universe map draws **owned ships** — cyan ring + dot at **`AtStar`** positions; **`InTransit`** shows a **dim line** plus a **bubble** along the route; HUD lists **ship count** and up to five ships with grid coordinates (ly).

### Changed

- **`spawn_starter_ship`:** the starter ship’s **`jump_ready_at`** is set to **spawn time** so the **battery is ready to warp immediately** (no initial recharge wait at the starter star).

- **`spawn_starter_ship` / `find_empty_red_dwarf_starter`:** after each random disk sample, searches a **50×50** integer grid **centered** on that anchor (row-major) for a **Red dwarf** with an **empty** first qualifying planet; improves hit rate vs. testing only the anchor cell. User-visible error text mentions the local grid when no slot is found.

### Fixed

- **Galaxy Explorer:** **scroll wheel** over the star info side panel no longer **zooms the map** (zoom applies only when the cursor is over the map); the whole info panel uses **one vertical scroll** instead of nested scroll regions fighting each other.

- **Explorer:** import **`spacetimedb_sdk::DbContext`** and generated **table / query** traits (`EmpireTableAccess`, `ShipTableAccess`, `empireQueryTableAccess`, …) so subscription and table iteration compile; use **`egui::TextEdit::singleline(...).desired_width(...)`** instead of chaining `desired_width` off `text_edit_singleline`’s response.
- **Module (WASM / Rust 2024):** starter disk sampling uses **`ReducerContext::random`** instead of **`Rng::gen`** (reserved keyword in Rust 2024).

### Performance

- **Galaxy Explorer** ([`explorer`](explorer/src/main.rs)): Universe map caches stars per 64×64 ly chunk instead of scanning every integer coordinate in view each frame; repaints only while panning, zooming, holding movement keys, or on pointer press (lower idle CPU).

### Breaking

- **`building` index** renamed **`building_by_garrison_owner`** → **`building_by_owner`**. Republish with **`--clear-database`** if you rely on old client bindings. **`BuildingKind`** derives **`Copy`** for reducer ergonomics.

- **Ship location:** docked ships use **`ShipLocation::AtStar` (`ShipAtStar { star_x, star_y }`)** only — **no `planet_index`**. Variant **`AtPlanet` / `ShipAtPlanet` removed**. Republish the module with **`spacetime publish ... --clear-database -y`** (or equivalent) so existing `ship` rows match the new `location` type.

- **Star coordinates:** `i32` grid values are now **tenths of a light-year** (0.1 ly per step), not 1 ly per step. Universe is a **500,000 ly** radius disk (~1M ly diameter). Density uses [`universe::settings`](universe/src/settings.rs): ~0.3 ly mean spacing at core, ~15 ly at edge, multiplied by **[`PLANE_DENSITY_SCALE`](universe/src/settings.rs) (≈ 10⁻⁶)** so the **2D** galaxy is ~1000× sparser per step than the prior single scaling (and far below a naive 3D-style count). Any persisted `star_x` / `star_y` or logic that assumed 1 ly per unit must be scaled or reinterpreted.

### Changed

- Root client default **`SPACETIMEDB_DB_NAME`** is now **`vast`** (was `my-db`) to match typical local publish names; set the env var if you use another database name.

- SpacetimeDB **`BuildingKind`**: new variant **`SalesDepot`** (government sink / instant exchange vendor; no new `Building` columns). Enum changes may require republishing with `--clear-database` if not auto-migrated. Sell reducer and pricing are not implemented yet.

- SpacetimeDB `ship` schema: new **`cargo`** column (`Vec<Material>`). Republish existing databases with `--clear-database` or run a migration if you keep data (no migration reducer added here).

- Procedural star density: **[`PLANE_DENSITY_SCALE`](universe/src/settings.rs)** set to **10⁻⁶** (2D grid; ~1000× sparser than the previous 10⁻³ factor) so expected galaxy population stays in a playable range.

- SpacetimeDB `building`: optional **`owner`** (`Option<Identity>`) and **`attack_mode`** (`Option<ShipAttackMode>`) for **military garrisons** only; other building kinds use `None`. Index `building_by_garrison_owner` on `owner`. Reducers do not enforce garrison invariants yet.
- [`ShipStats`](universe/src/ships.rs): `speed_tenths_ly_s` (`u32`) replaced by **`speed_lys` (`f64`)** — speed in light-years per second directly. Removed `speed_ly_per_s`; [`travel_duration_secs`](universe/src/ships.rs) now takes `speed_lys`.
- `MaterialKind` (Iron / Helium) lives in [`universe`](universe/src/resources.rs) with optional `SpacetimeType` via the `universe/spacetimedb` feature. [`Material`](universe/src/resources.rs) also derives `SpacetimeType` when that feature is enabled.
- **Semantics:** procedural `Material` uses `f64` as spawn **richness**; SpacetimeDB `building.warehouse_inventory` uses `Material` with `f64` as **stored quantity** (units). `building.mining_material` is `Option<Material>` (species + placeholder `f64` for future rates).
- Removed `InventoryEntry`; warehouse stock is `Vec<Material>` only.

### Added

- Workspace crate **[`vast-bindings`](vast-bindings/)**: SpacetimeDB Rust client bindings generated with **`spacetime generate --lang rust --out-dir vast-bindings/src --module-path spacetimedb`**. Library root is [`vast-bindings/src/mod.rs`](vast-bindings/src/mod.rs) (see `[lib] path` in [`vast-bindings/Cargo.toml`](vast-bindings/Cargo.toml)). Root [`vast`](src/main.rs) and [`explorer`](explorer/src/main.rs) depend on this crate; the old [`src/module_bindings`](src/module_bindings) tree was removed.
- Reducer **`spawn_starter_ship`**: requires a registered empire, idempotent if the player already has a ship; **randomly samples** disk anchors (via [`ReducerContext::random`](https://docs.rs/spacetimedb/latest/spacetimedb/struct.ReducerContext.html#method.random)) up to **4096** times in a **5000 ly** radius disk around the origin, and for each anchor scans a **50×50** grid for a **Red dwarf** with a planet that has **no buildings**, then inserts a **default** [`Ship`](spacetimedb/src/lib.rs) at **`ShipLocation::AtStar`** (star coordinates only). Returns an error if no slot is found after all samples.
- **Galaxy Explorer** bootstrap: on launch, connect to SpacetimeDB (defaults `SPACETIMEDB_HOST` = `http://127.0.0.1:3000`, `SPACETIMEDB_DB_NAME` = `vast`), **subscribe** to `empire` / `building` / `ship`, advance with **`frame_tick`** each frame; welcome screen for **empire name** then **`register_empire`** + **`spawn_starter_ship`**; HUD shows empire name and credits; map **centers** on the starter ship once (grid → ly via [`grid_to_ly`](universe/src/settings.rs)).

- [`universe::settings`](universe/src/settings.rs): `UNIVERSE_RADIUS_LY`, `COORD_UNITS_PER_LY` (0.1 ly per step), core/edge spacing, **`PLANE_DENSITY_SCALE`** for 2D sparsity; [`checker::expected_star_count_integral`](universe/src/checker.rs) sanity-checks total expected stars.
- **Galaxy Explorer** HUD: **Stars discovered** — total count of unique stars in chunk caches loaded this session (grows as you visit new map regions), not a theoretical “expected” total.
- SpacetimeDB `empire` table: `identity` (primary key), unique `name`, and `credits` (starting balance on registration).
- `register_empire` reducer to create an empire once per identity with validated name and starting credits.
- Root client (`src/main.rs`) subscribes to `empire`, registers via `EMPIRE_NAME` (default `Test Empire`), and logs new empire rows.
- SpacetimeDB `ship` table: `owner`, [`ShipStats`](universe/src/ships.rs) (includes `size_kt` capacity), **`cargo`** (`Vec<Material>` hold inventory, same quantity semantics as `building.warehouse_inventory`; capacity enforcement deferred to future reducers), [`ShipAttackMode`](universe/src/ships.rs), [`ShipLocation`](universe/src/ships.rs) (`AtStar` / `InTransit` with timestamps). Index `ship_by_owner`. No spawn/warp reducers yet.
- [`travel_duration_secs`](universe/src/ships.rs): `duration = distance_ly / speed_lys` using [`ShipStats::speed_lys`](universe/src/ships.rs) (light-years per second, `f64`).

### Removed

- Demo `person` table and `add` / `say_hello` reducers from the SpacetimeDB module.
