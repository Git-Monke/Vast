# Vast — Design Document
*Working title. API-only space MMO.*

---

## Core Concept

2D top-down space MMO where players control empires entirely via a publicly exposed API. No UI — players write bots in any language. A local translation server (Rust binary) bridges their bot to SpacetimeDB via WebSocket. Goal: get as rich as possible.

---

## Tech Stack

- **SpacetimeDB** — Rust modules, scheduled reducers, real-time subscriptions
- **Cargo workspace:**
  - `spacetimedb/` — server module
  - `universe/` — pure generation lib
  - `client/` — translation server
- No Axum — SpacetimeDB handles all networking

---

## Universe Generation

- Lazily generated, deterministic via seeded hash — stars only written to DB on first interaction
- 2D grid: **one integer step = 0.1 ly** (`i32` stores tenths of a light-year; 10 units = 1 ly)
- **Disk radius 500,000 ly** ( **1,000,000 ly diameter** ); no stars outside this radius
- Mean spacing between stars: **~0.3 ly** at the core, **~15 ly** at the disk edge (exponential in radius); per-cell probability `min(1, δ/spacing(r) × PLANE_DENSITY_SCALE)` with cell size `δ = 0.1` ly and **[`PLANE_DENSITY_SCALE`](universe/src/settings.rs) ≈ 10⁻⁶** so a **2D** map stays vastly sparser than a naive 3D-style fill
- Constants live in [`universe/src/settings.rs`](universe/src/settings.rs) (`CORE_MEAN_SPACING_LY`, `EDGE_MEAN_SPACING_LY`, etc.)
- Stars have random offset within their cell so distribution isn't grid-aligned *(not implemented in code yet)*
- Resource richness increases with distance from core

---

## Galaxy Map & Visibility

### Client-Side Star Positions
- The galaxy seed and density function are public — any client can compute the position of every star in the universe locally
- Star positions are free information; structure of the galaxy is known to everyone
- Zero DB load for position queries

### Radar Scanning
- What's *at* a star (planets, resources, buildings, military) requires active scanning
- A **Radar building** covers an X light-year radius around its planet
- Within radar range, the DB returns: planet types, resource richness, buildings present, military presence
- Outside radar range, a star is just a dot on the map
- Scan data freshness TBD — either persists until something changes, or expires and requires ongoing radar presence

---

## Planets

**Types:** Terrestrial, Desert, Gas Giant, Ice, Volcanic, Barren, Toxic

Each type has:
- Build cost modifier
- Resource type affinity
- Building slot count based on size
- Temperature rating (affects building degradation rate — hotter = faster decay)

---

## Economy

### Capital Injection
- Server acts as a government buyer — purchases basic resources at fixed baseline prices
- Credits enter the economy when resources are sold to this sink
- Players start with X credits to bootstrap early game

### Currency
- Credits only — resources are inputs to the production pipeline, not currency

### No Player Markets (MVP)
- All resource selling goes to the government sink at baseline prices
- Competition emerges from racing to extract and sell, and from blocking others' access
- Player markets are post-MVP

### Empire Upkeep
- Combined ship + building count past a threshold triggers compounding inefficiency penalty
- 5% per unit over threshold — soft cap on empire size
- No hard caps anywhere — all friction is economic

---

## Control & Military Presence

There is no ownership in Vast. There is only power projection.

- A planet is "yours" if your military is there and nobody else's is
- If a planet has no military presence, it is free for anyone to use or loot
- You cannot perform actions on a planet where a hostile military is present — their guns will engage you
- If you kill the military presence, the planet is open: you and anyone else can loot it, use buildings, or build — until someone establishes new military presence

### Military Units
- **Armed ships** — mobile presence, can be recalled or destroyed, presence ends immediately if they leave
- **Garrison building** — persistent presence, degrades like all buildings, survives without ships but vulnerable to degradation over time
- Both are needed for robust control: ships for flexibility, garrison for persistence

### Attack Modes
All ships and military buildings have two modes:
- **Defend** — engage only if a hostile unit attempts an action on the planet
- **Strike first** — engage any ship from an untrusted empire that enters the system

Strike first risks starting fights with neutrals; bots must manage trust lists carefully.

### Ship Spawning
- Ships can only be spawned on a planet that has both a **ship depot** and your **military presence**
- No military presence = you can't spawn there even if you built the depot

---

## Buildings

### General Rules
- Buildings are machines — no inherent ownership, they just operate
- Anyone can use or destroy buildings on a planet with no hostile military presence
- Buildings are destructible, but only after military presence on the planet is eliminated
- Buildings can be upgraded (level 1–10+) for better stats, increased durability, and higher upkeep

### Degradation System
- Every building degrades continuously, tracked as **% progress toward destruction**
- At 100% degradation the building is destroyed — server fires a subscription event
- Hotter planets degrade buildings faster
- Garrison buildings degrade slower than harvesters (exact ratio TBD)

### Repair
- A ship must be present at the planet to perform repairs
- Required minimum ship cargo capacity scales with building level:
  - Level 1 → 1kt
  - Level 10 → 50kt
  - (Scale TBD: linear or stepped)
- Repair costs credits only — no repair materials
- Cost = `building_base_cost × (degradation_percent)^2`
- Optimal play: repair before ~30–40% degradation

### Building Types (MVP)
| Building | Function |
|---|---|
| Mining Depot | Adds extraction **rate** (see **Star-system resources** below); no per-building stock |
| Ship Depot | Defines concurrent ship build **slots** (sum of levels); spawns ships (requires military presence) |
| Warehouse | Adds shared **capacity** at the star (`level × 1` kt per warehouse) |
| Sales Depot | Sells resources to government sink |
| Garrison | Provides persistent military presence and **military power** (scaled by level and degradation) |
| Radar | Reveals star system contents within X ly radius |

### Star-system resources (warehouse / mining)

- **No global mining ticks.** Production is **lazy**: amounts **materialize** only when something interacts (settlement path in reducers). Between interactions, the server stores **settled** kt, **`last_settled_at`**, and derives rates from buildings.
- **Shared warehouse** per star `(star_x, star_y)`: **`settled: Vec<Material>`** (kt per species, merged to one row per [`MaterialKind`](universe/src/resources.rs)), shared **`capacity_kt`** and **`last_settled_at`** (see `star_system_stock` table). **Capacity** = sum over all warehouses of **`level × 1` kt** (level 1 warehouse ⇒ 1 kt contribution).
- **Accrual:** per-kind rates aggregated into a map; `t_eff = min(Δt, remaining_kt / total_kt_s)` where **`remaining_kt = capacity − sum(settled)`** and **`total_kt_s`** is the sum of all miner rates at that star. If accrual would exceed capacity, amounts are **scaled proportionally** across materials. **Miner kt/s (per depot)** = `level × planet_richness × resource_richness × 0.01 × (1 − degradation)`; **resource_richness** is the vein multiplier on the targeted material.
- **Settlement** runs before changing miners/warehouses or **collecting** to ship. **`collect_star_resources(ship_id, pickup: Vec<Material>)`** settles, then subtracts requested kt per kind from **`settled`** and merges into **ship cargo** (subject to cargo capacity). Shared helpers live in [`universe::material_stock`](universe/src/material_stock.rs).
- **Military power** (abstract): sum over garrisons of `level × 100 × (1 − degradation)`.

---

## Ships

### Stats
- No fixed types — bots request properties (cargo capacity, speed, durability)
- Server returns credit cost, bot accepts or rejects
- Only spawnable at a planet with a ship depot and your military presence

### Travel
- `time = distance_ly × seconds_per_ly` (tunable constant, ~5s/ly)
- Warp cost: credits proportional to `distance × cargo weight`
- Ships in transit cannot change course
- Ships in transit are detectable by other ships within sensor radius — interception means positioning at the destination star before arrival

### Attack Modes
- Same as military buildings: **Defend** or **Strike first**

### Destruction
- Destroyed ships are gone permanently — no wreckage, no recovery

---

## Combat & PvP

- Military presence (ships + garrison) defends planets
- Buildings only destructible after military presence is eliminated
- Offense requires judgment — timing, positioning, political consequences
- Interception: detect enemy ships in transit via radar/sensors, position at destination, engage on arrival
- Neglected planets (degraded garrison, no ships) become open targets over time

---

## Neutron Stars

- 100x resource richness multiplier
- 10x credit + time cost to warp out (gravity well)
- Pulse on a per-star interval (~8–14 min with noise) destroys any ship in system
- Bots must time extraction windows carefully
- Special mechanics TBD

---

## Balancing Mechanics

- **Distance gradient** — core is cheap/safe/poor, outer rim is expensive/dangerous/rich
- **Superlinear upkeep** — past empire size threshold, costs compound
- **Building degradation** — neglect is punishing, forces active logistics
- **No ownership** — every asset is only as secure as your military; overextension is visible and exploitable
- **Radar coverage** — intelligence is earned; blind spots are real vulnerabilities

---

## MVP Scope

### Core Loop (must work end-to-end)
1. Register an empire (name, starting credits)
2. Compute galaxy map client-side, query scanned star data from DB
3. Warp a ship to a star
4. Establish military presence
5. Build on a planet
6. Spawn ships
7. Mine resources
8. Sell resources to government sink
9. Pay upkeep
10. Lose (run out of credits, can't pay upkeep)

### DB Tables Needed
- `Empire`
- `Ship` (includes **`cargo`**)
- `Building` (type, level, degradation %, planet slot, mining target, …)
- **`StarSystemStock`** (per star: settled kt per material, `last_settled_at`, cached `capacity_kt`)
- `Star` (written on first interaction — position is client-side, system data is DB) — *optional vs procedural-only*
- `ScanData` (what a given empire knows about a given star, with timestamp)

### Open Questions
- Scan data expiry — persists or expires over time?
- Exact degradation rate per planet temperature tier
- Building level → repair ship tonnage scale (linear vs stepped)
- Garrison degradation rate relative to other buildings
- Warehouse contents when warehouse is destroyed — gone, or briefly lootable?
- Upkeep tick frequency
- Trust list mechanics — how does an empire define trusted/neutral/hostile?
