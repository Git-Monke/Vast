# AI Context: Vast Project

This document provides a dense overview of the **Vast** codebase to help AI agents understand the architecture, key modules, and where to make changes without exhaustive searching.

## 🚀 Architecture Overview
Vast is a space simulation MMO built with **SpacetimeDB** (Rust backend) and a **Rust client** (with an `egui` explorer).

- **Backend (`spacetimedb/`)**: Authoritative game logic running as a WASM module in SpacetimeDB.
- **Universe Lib (`universe/`)**: Shared procedural generation logic. Star systems are derived deterministically from `(x, y)` coordinates and are **not** stored in the DB until interacted with.
- **Client (`src/`, `explorer/`)**: Connects to the DB via auto-generated bindings (`vast-bindings/`).

---

## 📂 Key Modules & Responsibilities

### Backend (`spacetimedb/src/`)
| File | Purpose |
| :--- | :--- |
| `schema.rs` | **CRITICAL**: Tables (`Empire`, `Ship`, `Building`, `StarSystemStock`) and Enums (`BuildingKind`, `Material`). |
| `building_reducers.rs` | Logic for `place_building`, `upgrade_building`. Handles costs/rules. |
| `ship_reducers.rs` | Ship lifecycle: `spawn_starter_ship`, `order_warp`, `complete_warp`. |
| `star_economy.rs` | **Lazy Settlement**: Resource accrual logic based on elapsed time. |
| `combat.rs` / `battle.rs` | Ship-to-ship and ship-to-garrison combat resolution. |
| `db_helpers.rs` | Common queries (e.g., `player_has_presence_at_star`). |

### Shared Library (`universe/src/`)
| File | Purpose |
| :--- | :--- |
| `generator.rs` | `generate_star(x, y)` -> Procedural star system data. |
| `ships.rs` | `ShipStats` and base ship definitions. |
| `resources.rs` | `MaterialKind` and resource definitions. |

---

## 🛠 Common Tasks: Where to Update

### 1. Adding a New Resource
1.  **`universe/src/resources.rs`**: Add to `MaterialKind` enum.
2.  **`universe/src/material_stock.rs`**: Update `material_from_kind_kt`.

### 2. Adding a New Building Type
1.  **`spacetimedb/src/schema.rs`**: Add to `BuildingKind` enum.
2.  **`spacetimedb/src/building_rules.rs`**: Define placement rules, costs, and effects.
3.  **`spacetimedb/src/building_reducers.rs`**: Ensure `place_building` handles any unique logic.

### 3. Modifying Ship Stats/Behavior
1.  **`universe/src/ships.rs`**: Update `ShipStats` struct or specific ship presets.
2.  **`spacetimedb/src/ship_reducers.rs`**: Update movement or action logic (e.g., `order_warp`).

### 4. Changing Combat Logic
1.  **`spacetimedb/src/battle.rs`**: Modify `run_battle` or damage calculation logic.

---

## ⚠️ Critical Patterns & Gotchas

-   **Lazy State**: Stars aren't in the DB by default. Use `universe::generator::generate_star(x, y)` to get "static" star data.
-   **Resource Settlement**: **ALWAYS** call `settle_star_resources` (in `star_economy.rs`) before modifying `StarSystemStock` or `Building` levels. This ensures "lazy" production since the last interaction is accounted for.
-   **Scheduled Reducers**: Movement (`order_warp`) uses SpacetimeDB's scheduler to trigger `complete_warp` automatically.
-   **Bindings**: If you change `spacetimedb/src/schema.rs` or reducers, you **must** regenerate bindings in `vast-bindings/` (usually via `Makefile` or `spacetimedb generate`).

---

## 🔍 Search Keywords
- `reducer`: Find entry points for game actions.
- `spacetimedb::table`: Find data structures.
- `generate_star`: Find procedural generation logic.
- `settle_star_resources`: Find economy heartbeat logic.
