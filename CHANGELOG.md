# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed

- **SpacetimeDB — scanning:** `complete_scan` now deletes any existing `ScanResult` for the same empire and star location before inserting a new one, ensuring users only fetch and store the latest data for a system.
- **Galaxy Explorer:** fixed a bug where radar scanning would trigger every frame once a radar was present and a target star ID was entered. Added a "Scan from radar" button with a proper click check to prevent excessive database requests.
- **Battle simulation (`run_battle`):** units with no damage row were treated as dead, so the battle loop never ran. Missing `CombatantResult` now means **undamaged** (still alive). Unit tests cover damage resolution, defender speed selection, stacked team attack, stalemate, and garrison stats.

### Added

- **SpacetimeDB — combat at stars (two-team battle):** **`resolve_battle_at_star`** runs **`run_battle`** for the aggressor’s docked ships + their garrisons vs everyone else at **`(star_x, star_y)`**, then applies damage. **`complete_warp`** triggers a battle when the arriving ship is **`StrikeFirst`** and other players’ ships are docked there. **`set_ship_attack_mode(ship_id, attack_mode)`** updates mode and triggers the same battle when switching to **`StrikeFirst`** with others present. **`collect_star_resources`** and **`sell_star_warehouse`** call **`resolve_battle_at_star`** first when an **enemy military garrison** is at the star, then **return without** settling/collecting or selling in that same reducer call so the client can read combat results and retry. **`execute_battle`** still resolves combat on demand. Regenerate **`vast-bindings`** after pulling so clients see **`set_ship_attack_mode`**.

- **SpacetimeDB — Sales Depot (government sink):** baseline credits per kt in [`universe::resources`](universe/src/resources.rs) (**Iron 10**, **Helium 15**). Reducers **`sell_ship_cargo(ship_id, amounts)`** and **`sell_star_warehouse(ship_id, amounts)`** require the caller’s ship **`AtStar`** and **any** **`SalesDepot`** at that **`(star_x, star_y)`**; **`amounts`** empty means sell **everything** from ship cargo or from settled warehouse stock (after **`settle_star_resources`** for warehouse). Credits are added to **`Empire.credits`** (`u64` saturating). **`universe::credits_for_materials_sale`** matches server payout (per-kind `floor(kt × price)`).
- **Galaxy Explorer:** when a selected star has a **Sales Depot**, **Sales (Sales Depot)** shows baseline prices, sliders + **Sell from ship** / **Sell all ship cargo**, and warehouse sliders + **Sell warehouse** / **Sell all warehouse stock** (uses the same ship selection as collect).

- **SpacetimeDB — star-system economy (lazy mining):** public table **`star_system_stock`** (`star_location_id`, `last_settled_at`, `capacity_kt`, **`settled: Vec<Material>`**). Mining accrues only on **settlement** (no scheduled mining ticks): `t_eff = min(Δt, remaining_kt / total_kt_s)` with proportional clamp to capacity; miner rate per depot = `level × planet_richness × resource_richness × 0.01 × (1 − degradation)`; warehouse capacity = **Σ (warehouse level × 1 kt)** per star. Reducer **`collect_star_resources(ship_id, pickup)`** (ship must be **`AtStar`**) settles then loads requested materials onto **ship cargo** (enforces cargo capacity). **`place_building`** / **`upgrade_building`** call settlement before mutating buildings. Helpers in [`spacetimedb/src/star_economy.rs`](spacetimedb/src/star_economy.rs), [`universe::material_stock`](universe/src/material_stock.rs), and [`spacetimedb/src/building_rules.rs`](spacetimedb/src/building_rules.rs).
- **Galaxy Explorer:** subscribes to **`star_system_stock`**; **Star economy** panel shows capacity, per-material mining rates, military power, ship build slots, settled/available kt; **Collect from warehouse** builds a **`Vec<Material>`** and calls **`collect_star_resources`**.
- **[`universe::star_location_id`](universe/src/star_id.rs):** 128-bit id for a star cell (matches server `star_location_id`).
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

- **Breaking — SpacetimeDB `Ship` table:** removed the **`location`** column (`ShipLocation` sum type). Rows now use **`in_transit`**, **`star_x`** / **`star_y`** (current cell when docked; **destination** while in transit, matching **`transit_to_*`**), **`transit_from_x/y`**, **`transit_to_x/y`**, **`transit_depart_at`**, **`transit_arrive_at`**, and existing **`jump_ready_at`** / **`health`**. Btree index **`ship_by_docked_star`** on **`[in_transit, star_x, star_y]`** supports looking up ships by star cell (docked vs inbound). **[`universe::ship_location_from_flat`](universe/src/ships.rs)** maps flat columns back to **`ShipLocation`** for callers that still want the enum. Republish with **`--clear-database`** if you have an existing module DB. Regenerate **`vast-bindings`** with **`spacetime generate`**.

- **[`universe::Material`](universe/src/resources.rs):** **`amount()`** returns the inner `f64`; **`multiplier()`** delegates to it. **[`material_stock`](universe/src/material_stock.rs)** uses **`amount()`** where only the payload matters. **Galaxy Explorer** [`building_economy`](explorer/src/building_economy.rs) uses **`binding_to_universe`** / **`universe_to_binding`** and **`Material::kind()`** for mining rates and pickup so new ores extend those two conversions (plus **`MaterialKind::ALL`**) instead of scattered Iron/Helium matches.

- **`spawn_starter_ship`:** the starter ship’s **`jump_ready_at`** is set to **spawn time** so the **battery is ready to warp immediately** (no initial recharge wait at the starter star).

- **`spawn_starter_ship` / `find_empty_red_dwarf_starter`:** after each random disk sample, searches a **50×50** integer grid **centered** on that anchor (row-major) for a **Red dwarf** with an **empty** first qualifying planet; improves hit rate vs. testing only the anchor cell. User-visible error text mentions the local grid when no slot is found.

### Fixed

- **Galaxy Explorer:** realtime readouts (ship **in-transit** bubble, star-economy **theoretical** stock using wall-clock time) no longer stall when the map is **idle** — the app now **`request_repaint()`** on the Universe tab while **connected** so `sync_session` / `frame_tick` run on a steady cadence (previously repaints were only requested on map drag/scroll/keys). **Debug** builds **`eprintln!`** when **`frame_tick`** exceeds **16 ms** to spot slow SpacetimeDB client work.

- **Galaxy Explorer:** **scroll wheel** over the star info side panel no longer **zooms the map** (zoom applies only when the cursor is over the map); the whole info panel uses **one vertical scroll** instead of nested scroll regions fighting each other.

- **Explorer:** import **`spacetimedb_sdk::DbContext`** and generated **table / query** traits (`EmpireTableAccess`, `ShipTableAccess`, `empireQueryTableAccess`, …) so subscription and table iteration compile; use **`egui::TextEdit::singleline(...).desired_width(...)`** instead of chaining `desired_width` off `text_edit_singleline`’s response.
- **Module (WASM / Rust 2024):** starter disk sampling uses **`ReducerContext::random`** instead of **`Rng::gen`** (reserved keyword in Rust 2024).

### Performance

- **Galaxy Explorer** ([`explorer`](explorer/src/main.rs)): Universe map caches stars per 64×64 ly chunk instead of scanning every integer coordinate in view each frame. While **not** connected, repaints are still driven mainly by map interaction (lower idle CPU). While **connected**, repaints run continuously so replicated state stays live. The selected-star side panel **throttles** full-table **`building`** scans to **150 ms** per star; **`star_system_stock`** for the current star is refreshed **every frame** (typically a small table).

### Notes (developer)

- **Galaxy Explorer / SpacetimeDB:** moving **`frame_tick`** off the UI thread via **`run_threaded()`** and callback-driven snapshots ([`AGENTS.md`](AGENTS.md)) remains a follow-up if **`frame_tick`** profiling shows sustained multi-ms stalls after the repaint fix.

### Breaking

- **`star_system_stock` / `collect_star_resources`:** replaced **`settled_iron_kt`** / **`settled_helium_kt`** with **`settled: Vec<Material>`**. **`collect_star_resources`** now takes **`pickup: Vec<Material>`** instead of two `f64` arguments. Regenerate client bindings (`spacetime generate …`), update clients, and republish with **`--clear-database`** when adopting.

- **SpacetimeDB `building`:** removed **`warehouse_inventory`** (shared stock lives on **`star_system_stock`**). Regenerate bindings and republish with **`--clear-database`** as needed.

- **`building` index** renamed **`building_by_garrison_owner`** → **`building_by_owner`**. Republish with **`--clear-database`** if you rely on old client bindings. **`BuildingKind`** derives **`Copy`** for reducer ergonomics.

- **Ship location:** docked ships use **`ShipLocation::AtStar` (`ShipAtStar { star_x, star_y }`)** only — **no `planet_index`**. Variant **`AtPlanet` / `ShipAtPlanet` removed**. Republish the module with **`spacetime publish ... --clear-database -y`** (or equivalent) so existing `ship` rows match the new `location` type.

- **Star coordinates:** `i32` grid values are now **tenths of a light-year** (0.1 ly per step), not 1 ly per step. Universe is a **500,000 ly** radius disk (~1M ly diameter). Density uses [`universe::settings`](universe/src/settings.rs): ~0.3 ly mean spacing at core, ~15 ly at edge, multiplied by **[`PLANE_DENSITY_SCALE`](universe/src/settings.rs) (≈ 10⁻⁶)** so the **2D** galaxy is ~1000× sparser per step than the prior single scaling (and far below a naive 3D-style count). Any persisted `star_x` / `star_y` or logic that assumed 1 ly per unit must be scaled or reinterpreted.

### Changed

- Root client default **`SPACETIMEDB_DB_NAME`** is now **`vast`** (was `my-db`) to match typical local publish names; set the env var if you use another database name.

- SpacetimeDB **`BuildingKind`**: new variant **`SalesDepot`** (government sink / instant exchange vendor; no new `Building` columns). Enum changes may require republishing with `--clear-database` if not auto-migrated.

- SpacetimeDB `ship` schema: new **`cargo`** column (`Vec<Material>`). Republish existing databases with `--clear-database` or run a migration if you keep data (no migration reducer added here).

- Procedural star density: **[`PLANE_DENSITY_SCALE`](universe/src/settings.rs)** set to **10⁻⁶** (2D grid; ~1000× sparser than the previous 10⁻³ factor) so expected galaxy population stays in a playable range.

- SpacetimeDB `building`: optional **`owner`** (`Option<Identity>`) and **`attack_mode`** (`Option<ShipAttackMode>`) for **military garrisons** only; other building kinds use `None`. Index `building_by_garrison_owner` on `owner`. Reducers do not enforce garrison invariants yet.
- [`ShipStats`](universe/src/ships.rs): `speed_tenths_ly_s` (`u32`) replaced by **`speed_lys` (`f64`)** — speed in light-years per second directly. Removed `speed_ly_per_s`; [`travel_duration_secs`](universe/src/ships.rs) now takes `speed_lys`.
- `MaterialKind` (Iron / Helium) lives in [`universe`](universe/src/resources.rs) with optional `SpacetimeType` via the `universe/spacetimedb` feature. [`Material`](universe/src/resources.rs) also derives `SpacetimeType` when that feature is enabled.
- **Semantics:** procedural `Material` uses `f64` as spawn **richness**; star-system **stored** quantities are **`star_system_stock.settled`** (and ship **`cargo`**). `building.mining_material` is `Option<Material>` (species + vein **richness** multiplier).

### Added

- Workspace crate **[`vast-bindings`](vast-bindings/)**: SpacetimeDB Rust client bindings generated with **`spacetime generate --lang rust --out-dir vast-bindings/src --module-path spacetimedb`**. Library root is [`vast-bindings/src/mod.rs`](vast-bindings/src/mod.rs) (see `[lib] path` in [`vast-bindings/Cargo.toml`](vast-bindings/Cargo.toml)). Root [`vast`](src/main.rs) and [`explorer`](explorer/src/main.rs) depend on this crate; the old [`src/module_bindings`](src/module_bindings) tree was removed.
- Reducer **`spawn_starter_ship`**: requires a registered empire, idempotent if the player already has a ship; **randomly samples** disk anchors (via [`ReducerContext::random`](https://docs.rs/spacetimedb/latest/spacetimedb/struct.ReducerContext.html#method.random)) up to **4096** times in a **5000 ly** radius disk around the origin, and for each anchor scans a **50×50** grid for a **Red dwarf** with a planet that has **no buildings**, then inserts a **default** [`Ship`](spacetimedb/src/lib.rs) at **`ShipLocation::AtStar`** (star coordinates only). Returns an error if no slot is found after all samples.
- **Galaxy Explorer** bootstrap: on launch, connect to SpacetimeDB (defaults `SPACETIMEDB_HOST` = `http://127.0.0.1:3000`, `SPACETIMEDB_DB_NAME` = `vast`), **subscribe** to `empire` / `building` / `ship`, advance with **`frame_tick`** each frame; welcome screen for **empire name** then **`register_empire`** + **`spawn_starter_ship`**; HUD shows empire name and credits; map **centers** on the starter ship once (grid → ly via [`grid_to_ly`](universe/src/settings.rs)).

- [`universe::settings`](universe/src/settings.rs): `UNIVERSE_RADIUS_LY`, `COORD_UNITS_PER_LY` (0.1 ly per step), core/edge spacing, **`PLANE_DENSITY_SCALE`** for 2D sparsity; [`checker::expected_star_count_integral`](universe/src/checker.rs) sanity-checks total expected stars.
- **Galaxy Explorer** HUD: **Stars discovered** — total count of unique stars in chunk caches loaded this session (grows as you visit new map regions), not a theoretical “expected” total.
- SpacetimeDB `empire` table: `identity` (primary key), unique `name`, and `credits` (starting balance on registration).
- `register_empire` reducer to create an empire once per identity with validated name and starting credits.
- Root client (`src/main.rs`) subscribes to `empire`, registers via `EMPIRE_NAME` (default `Test Empire`), and logs new empire rows.
- SpacetimeDB `ship` table: `owner`, [`ShipStats`](universe/src/ships.rs) (includes `size_kt` capacity), **`cargo`** (`Vec<Material>` hold inventory in kt; **`collect_star_resources`** enforces capacity), [`ShipAttackMode`](universe/src/ships.rs), [`ShipLocation`](universe/src/ships.rs) (`AtStar` / `InTransit` with timestamps). Index `ship_by_owner`. No spawn/warp reducers yet.
- [`travel_duration_secs`](universe/src/ships.rs): `duration = distance_ly / speed_lys` using [`ShipStats::speed_lys`](universe/src/ships.rs) (light-years per second, `f64`).

### Removed

- Demo `person` table and `add` / `say_hello` reducers from the SpacetimeDB module.
