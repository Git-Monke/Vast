use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table};
use universe::generator::{generate_star, star_info_at, StarType};
use universe::settings::COORD_UNITS_PER_LY;
use universe::Material;
use universe::{ShipAtStar, ShipAttackMode, ShipLocation, ShipStats};

/// Credits granted when an empire first registers.
const STARTING_CREDITS: u64 = 10_000;
const MAX_EMPIRE_NAME_LEN: usize = 64;

/// New players spawn at a random point within this Euclidean radius (light-years) of the galactic origin.
const STARTER_DISK_RADIUS_LY: f64 = 5_000.0;

/// Random disk samples tried before giving up (empty Red dwarf + planet with no buildings is sparse).
const MAX_STARTER_SAMPLE_ATTEMPTS: u32 = 4_096;

/// After each disk sample, search this many cells per side (centered on the sample anchor).
const STARTER_LOCAL_GRID: i32 = 50;
const STARTER_LOCAL_HALF: i32 = STARTER_LOCAL_GRID / 2;

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum BuildingKind {
    MiningDepot,
    Warehouse,
    MilitaryGarrison,
    SalesDepot,
    ShipDepot,
}

/// Stable procedural planet key for hashing and logging (no `planet` table row).
///
/// Bit layout (lossless for `i32` coordinates and `u8` planet slot):
/// - Bits `0..32`: `star_x` reinterpreted as `u32` (two's complement bits).
/// - Bits `32..64`: `star_y` reinterpreted as `u32`.
/// - Bits `64..72`: `planet_index` as `u8`.
/// - Bits `72..128`: zero.
#[must_use]
pub fn planet_location_id(star_x: i32, star_y: i32, planet_index: u8) -> u128 {
    let x = u128::from(star_x as u32);
    let y = u128::from(star_y as u32);
    let p = u128::from(planet_index);
    x | (y << 32) | (p << 64)
}

#[spacetimedb::table(accessor = empire, public)]
pub struct Empire {
    #[primary_key]
    identity: Identity,
    #[unique]
    name: String,
    credits: u64,
}

#[spacetimedb::table(
    accessor = building,
    public,
    index(
        accessor = building_by_planet_location,
        btree(columns = [star_x, star_y, planet_index])
    ),
    index(
        accessor = building_by_planet_slot,
        btree(columns = [star_x, star_y, planet_index, slot_index])
    ),
    index(accessor = building_by_garrison_owner, btree(columns = [owner]))
)]
pub struct Building {
    #[primary_key]
    #[auto_inc]
    id: u64,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
    slot_index: u8,
    kind: BuildingKind,
    level: u32,
    degradation_percent: f32,
    /// Which vein this depot targets; `f64` reserved for future rate/amount semantics.
    mining_material: Option<Material>,
    /// Stock per species; `f64` is quantity in units (not procedural richness).
    warehouse_inventory: Vec<Material>,
    /// Military garrison only: empire that operates this garrison. Must be `None` for other kinds.
    owner: Option<Identity>,
    /// Military garrison only: same semantics as [`Ship::attack_mode`]. Must be `None` for other kinds.
    attack_mode: Option<ShipAttackMode>,
}

#[spacetimedb::table(
    accessor = ship,
    public,
    index(accessor = ship_by_owner, btree(columns = [owner]))
)]
pub struct Ship {
    #[primary_key]
    #[auto_inc]
    id: u64,
    owner: Identity,
    stats: ShipStats,
    /// Hold contents; `f64` on each [`Material`] variant is quantity (units), same as [`Building::warehouse_inventory`].
    /// Total load must stay within [`ShipStats::size_kt`] when reducers enforce logistics.
    cargo: Vec<Material>,
    attack_mode: ShipAttackMode,
    location: ShipLocation,
}

#[spacetimedb::reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    // Called when the module is initially published
}

#[spacetimedb::reducer(client_connected)]
pub fn identity_connected(_ctx: &ReducerContext) {
    // Called everytime a new client connects
}

#[spacetimedb::reducer(client_disconnected)]
pub fn identity_disconnected(_ctx: &ReducerContext) {
    // Called everytime a client disconnects
}

#[spacetimedb::reducer]
pub fn register_empire(ctx: &ReducerContext, name: String) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Empire name cannot be empty".to_string());
    }
    if trimmed.len() > MAX_EMPIRE_NAME_LEN {
        return Err(format!(
            "Empire name must be at most {} characters",
            MAX_EMPIRE_NAME_LEN
        ));
    }

    if ctx.db.empire().identity().find(ctx.sender()).is_some() {
        return Err("Empire already registered for this identity".to_string());
    }

    if ctx
        .db
        .empire()
        .name()
        .find(&trimmed.to_string())
        .is_some()
    {
        return Err("That empire name is already taken".to_string());
    }

    ctx.db.empire().insert(Empire {
        identity: ctx.sender(),
        name: trimmed.to_string(),
        credits: STARTING_CREDITS,
    });

    Ok(())
}

fn owner_has_any_ship(ctx: &ReducerContext, owner: Identity) -> bool {
    ctx.db.ship().iter().any(|s| s.owner == owner)
}

fn planet_has_any_building(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
) -> bool {
    ctx.db.building().iter().any(|b| {
        b.star_x == star_x && b.star_y == star_y && b.planet_index == planet_index
    })
}

/// Uniform random point in a disk (area-uniform): radius `r_grid` in grid units (tenths of a ly).
/// Uses [`ReducerContext::random`] so we avoid `gen` (reserved in Rust 2024) and stay WASM-safe.
fn sample_disk_grid_point(ctx: &ReducerContext, r_grid: f64) -> (i32, i32) {
    let u1: f64 = ctx.random();
    let u2: f64 = ctx.random();
    let theta = 2.0 * std::f64::consts::PI * u1;
    let r = r_grid * u2.sqrt();
    let sx = (r * theta.cos()).round() as i32;
    let sy = (r * theta.sin()).round() as i32;
    (sx, sy)
}

/// Row-major scan of a [`STARTER_LOCAL_GRID`]×[`STARTER_LOCAL_GRID`] region centered on `(anchor_x, anchor_y)`.
fn try_find_starter_in_local_grid(
    ctx: &ReducerContext,
    anchor_x: i32,
    anchor_y: i32,
) -> Option<(i32, i32)> {
    for oy in 0..STARTER_LOCAL_GRID {
        for ox in 0..STARTER_LOCAL_GRID {
            let gx = anchor_x + ox - STARTER_LOCAL_HALF;
            let gy = anchor_y + oy - STARTER_LOCAL_HALF;
            let Some((star_type, _)) = star_info_at(gx, gy) else {
                continue;
            };
            if star_type != StarType::Red {
                continue;
            }
            let Some(sys) = generate_star(gx, gy) else {
                continue;
            };
            for p in sys.planets {
                if planet_has_any_building(ctx, gx, gy, p.index) {
                    continue;
                }
                return Some((gx, gy));
            }
        }
    }
    None
}

fn find_empty_red_dwarf_starter(ctx: &ReducerContext) -> Option<(i32, i32)> {
    let r_grid = STARTER_DISK_RADIUS_LY * f64::from(COORD_UNITS_PER_LY);
    for _ in 0..MAX_STARTER_SAMPLE_ATTEMPTS {
        let (ax, ay) = sample_disk_grid_point(ctx, r_grid);
        if let Some(found) = try_find_starter_in_local_grid(ctx, ax, ay) {
            return Some(found);
        }
    }
    None
}

#[spacetimedb::reducer]
pub fn spawn_starter_ship(ctx: &ReducerContext) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    if owner_has_any_ship(ctx, ctx.sender()) {
        return Ok(());
    }

    let Some((star_x, star_y)) = find_empty_red_dwarf_starter(ctx) else {
        return Err(
            "No empty starter planet on a Red dwarf found after searching the starter disk (50×50 grid per sample)"
                .to_string(),
        );
    };

    ctx.db.ship().insert(Ship {
        id: 0,
        owner: ctx.sender(),
        stats: ShipStats::default(),
        cargo: vec![],
        attack_mode: ShipAttackMode::Defend,
        location: ShipLocation::AtStar(ShipAtStar { star_x, star_y }),
    });

    Ok(())
}
