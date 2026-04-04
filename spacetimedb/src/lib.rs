use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table};
use universe::Material;
use universe::{ShipAttackMode, ShipLocation, ShipStats};

/// Credits granted when an empire first registers.
const STARTING_CREDITS: u64 = 10_000;
const MAX_EMPIRE_NAME_LEN: usize = 64;

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
