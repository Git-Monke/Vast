# vast-sdk — TypeScript SDK for Vast

API wrapper around SpacetimeDB auto-generated bindings, designed for bot authors.

---

## Overview

- **Design:** Object-oriented. Data wrapped in classes with methods, not flat structs.
- **Pure universe helpers:** `Stars.get(x, y)` and `Stars.get(x, y).planet(index)` are procedural — no DB needed.
- **Auto-subscribe:** `VastDB.connect()` subscribes to all tables automatically.
- **Void reducer calls:** `ship.moveTo()` etc. return `void` — reducer errors logged internally.

---

## File Structure

```
vast-sdk/
├── package.json
├── tsconfig.json
├── README.md
├── src/
│   ├── VastDB.ts              # Main entry point
│   ├── types.ts               # Re-exports from module_bindings
│   ├── objects/
│   │   ├── Ship.ts
│   │   ├── Building.ts
│   │   ├── Empire.ts
│   │   ├── PlayerPresence.ts
│   │   ├── ScanResult.ts
│   │   ├── ScanJob.ts
│   │   └── StarSystemStock.ts
│   ├── universe/              # Procedural generation helpers
│   │   ├── Stars.ts           # Stars.get(x, y) → Star
│   │   ├── Star.ts            # .planet(index) → Planet
│   │   └── Planet.ts           # Procedural planet data
│   ├── events/
│   │   └── VastEventEmitter.ts
│   ├── module_bindings/       # AUTO-GENERATED (regenerate with spacetime CLI)
│   │   └── ...
│   └── index.ts
```

Regenerate bindings:
```bash
spacetime generate --lang typescript --out-dir vast-sdk/src/module_bindings --module-path spacetimedb
```

---

## Phase 1: Project Setup

1. `package.json` — name `vast-sdk`, peer dep on `spacetimedb`, build with `tsc` or `rollup`
2. `tsconfig.json`
3. Document regeneration command in `README.md`

---

## Phase 2: Universe Helpers (`universe/`)

These are **pure** — same seed hash as server, no network calls.

### `Stars`
```typescript
class Stars {
  static get(x: number, y: number): Star;
}
```

### `Star`
```typescript
class Star {
  readonly x: number;
  readonly y: number;
  readonly id: string;           // e.g. "LL-rx-ry"
  readonly starKind: StarKind;   // RedDwarf, YellowSun, BlueGiant, NeutronStar, BlackHole

  // Get planet by index and planetGeneratorKey (from PlayerPresence at this star)
  // If key is not provided, looks up PlayerPresence internally via db reference
  planet(index: number, key?: bigint): Planet | null;

  // DB data at this star (filtered from subscribed tables)
  buildings(): Building[];
  stock(): StarSystemStock | null;
  playerPresences(): PlayerPresence[];
}
```

### `Planet`
```typescript
class Planet {
  readonly index: number;
  readonly planetGeneratorKey: bigint;  // needed for procedural generation
  readonly name: string;
  readonly temperatureK: number;
  readonly planetType: PlanetType;
  readonly size: number;
  readonly richness: number;
  readonly resources: Material[];  // from procedural generation

  // Building info at this planet (from subscribed buildings table, slot = index)
  buildlings(): Building[];
}
```

---

## Phase 3: `VastDB` Main Class

```typescript
class VastDB {
  private conn: DbConnection;

  static async connect(options?: {
    host?: string;
    dbName?: string;
    empireName?: string;
  }): Promise<VastDB>;

  tick(): void;

  identity(): Uint8Array | null;

  // Direct table access (raw subscribed data)
  readonly ships: Ship[];
  readonly buildings: Building[];
  readonly empire: Empire | null;
  readonly playerPresences: PlayerPresence[];
  readonly scanResults: ScanResult[];
  readonly scanJobs: ScanJob[];
  readonly starSystemStocks: StarSystemStock[];

  // Universe helpers (procedural)
  readonly stars: typeof Stars;

  // Planet key lookup — get planetGeneratorKey for a star from PlayerPresence
  planetKey(x: number, y: number): bigint | null;
}
```

---

## Phase 4: Object Wrappers

### `Ship`
```typescript
class Ship {
  readonly id: bigint;
  readonly owner: Uint8Array;
  readonly stats: ShipStats;
  readonly attackMode: ShipAttackMode;
  readonly health: number;

  atStar(): boolean;
  inTransit(): boolean;
  currentStar(): { x: number; y: number };
  destination(): { x: number; y: number } | null;
  cargo(): Material[];

  moveTo(x: number, y: number): void;
  setAttackMode(mode: ShipAttackMode): void;

  // If pickup is empty, collect all cargo
  collect(pickup?: Material[]): void;
  sellCargo(amounts?: Material[]): void;  // if amounts empty, sell all
}
```

### `Building`
```typescript
class Building {
  readonly id: bigint;
  readonly starX: number;
  readonly starY: number;
  readonly planetIndex: number;
  readonly slotIndex: number;
  readonly kind: BuildingKind;
  readonly level: number;
  readonly degradationPercent: number;
  readonly owner: Uint8Array | null;
  readonly attackMode: ShipAttackMode | null;
  readonly health: number;

  upgrade(): void;
  repair(): void;
}
```

### `Empire`
```typescript
class Empire {
  readonly identity: Uint8Array;
  readonly name: string;
  readonly credits: bigint;
}
```

### `PlayerPresence`
```typescript
class PlayerPresence {
  readonly id: bigint;
  readonly starX: number;
  readonly starY: number;
  readonly empireId: Uint8Array;
  readonly planetGeneratorKey: bigint;
}
```

### `ScanResult`
```typescript
class ScanResult {
  readonly id: bigint;
  readonly empireId: Uint8Array;
  readonly starX: number;
  readonly starY: number;
  readonly planetGeneratorKey: bigint;
  readonly planets: ScannedPlanet[];
  readonly dockedShips: ScannedDockedShip[];
}
```

### `ScanJob`
```typescript
class ScanJob {
  readonly scheduledId: bigint;
  readonly scheduledAt: Date;
  readonly empireId: Uint8Array;
  readonly scanInitiator: ScanInitiator;
  readonly initiatorId: bigint;
  readonly shipId: bigint;
  readonly toStarX: number;
  readonly toStarY: number;
}
```

### `StarSystemStock`
```typescript
class StarSystemStock {
  readonly starLocationId: bigint;
  readonly starX: number;
  readonly starY: number;
  readonly lastSettledAt: Date;
  readonly capacityKt: number;
  readonly settled: Material[];

  // If amounts empty, sell all
  sell(amounts?: Material[]): void;
}
```

---

## Phase 5: Event System

CRUD-style events, fired during `tick()`:

```typescript
// Ship events
onShipCreated(cb: (ship: Ship) => void): void;
onShipUpdated(cb: (ship: Ship, prev: Ship) => void): void;
onShipDeleted(cb: (shipId: bigint) => void): void;

// Building events
onBuildingPlaced(cb: (building: Building) => void): void;
onBuildingUpgraded(cb: (building: Building, prev: Building) => void): void;
onBuildingDegraded(cb: (building: Building, prev: Building) => void): void;

// Empire events
onEmpireUpdated(cb: (empire: Empire, prev: Empire) => void): void;

// Scan events
onScanCompleted(cb: (result: ScanResult) => void): void;
onScanJobScheduled(cb: (job: ScanJob) => void): void;

// PlayerPresence events (empire enters/leaves a star system)
onPresenceUpdated(cb: (presence: PlayerPresence, prev: PlayerPresence | null) => void): void;
onPresenceRemoved(cb: (presenceId: bigint) => void): void;

// StarSystemStock events
onStockUpdated(cb: (stock: StarSystemStock, prev: StarSystemStock | null) => void): void;
```

---

## Phase 6: Internal Reducer Mapping

Used by object methods — not exposed directly:

| Object Method | Reducer Called |
|---|---|
| `Ship.moveTo()` | `orderWarp({ shipId, destStarX, destStarY })` |
| `Ship.setAttackMode()` | `setShipAttackMode({ shipId, attackMode })` |
| `Ship.collect()` | `collectStarResources({ shipId, pickup })` |
| `Ship.sellCargo()` | `sellShipCargo({ shipId, amounts })` |
| `Building.upgrade()` | `upgradeBuilding({ buildingId })` |
| `StarSystemStock.sell()` | `sellStarWarehouse({ shipId, amounts })` |
| New empire | `registerEmpire({ name })` |
| New ship | `spawnStarterShip()` |
| New scan | `initiateScan({ ... })` |

---

## Notes

1. **`executeBattle` excluded** — battle resolves automatically via `orderWarp` / `setShipAttackMode` triggers.
2. **`Stars.get()` is pure** — uses same seed hash as server, no network.
3. **Planet generation requires `planetGeneratorKey`** — `Star.planet(index, key?)` uses `key` if provided, otherwise looks up `PlayerPresence` at that star to get the key. Players can cache keys locally without needing a `ScanResult`.
4. **`Planet.buildlings()`** — returns buildings at this planet slot from subscribed tables.
5. **TypeScript `bigint`** — SDK uses `bigint` for IDs (matches Rust `u64`).
6. **`collect(pickup?: Material[])`** — if empty, collects all cargo.
7. **`sell(amounts?: Material[])`** — if empty, sells all stock/cargo.
8. **`db.planetKey(x, y)`** — returns `planetGeneratorKey` from `PlayerPresence` at that star, or `null` if no presence recorded.
