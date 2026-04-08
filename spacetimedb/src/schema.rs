use spacetimedb::{Identity, ScheduleAt, SpacetimeType, Timestamp, ViewContext};
use universe::{Material, ShipAttackMode, ShipStats};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum BuildingKind {
    MiningDepot,
    Warehouse,
    MilitaryGarrison,
    SalesDepot,
    ShipDepot,
    Radar,
}

pub(crate) fn player_has_presence_at_star(
    ctx: &ViewContext,
    player: Identity,
    star_x: i32,
    star_y: i32,
) -> bool {
    ctx.db
        .player_presence()
        .presence_by_star_and_empire()
        .filter((star_x, star_y, player))
        .next()
        .is_some()
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

/// Stable id for a star cell (no planet bits); used by [`StarSystemStock`].
#[must_use]
pub fn star_location_id(star_x: i32, star_y: i32) -> u128 {
    let x = u128::from(star_x as u32);
    let y = u128::from(star_y as u32);
    x | (y << 32)
}

#[spacetimedb::table(accessor = empire, public)]
pub struct Empire {
    #[primary_key]
    pub identity: Identity,
    #[unique]
    pub name: String,
    pub credits: u64,
}

#[spacetimedb::table(
    accessor = building,
    index(
        accessor = building_by_planet_location,
        btree(columns = [star_x, star_y, planet_index])
    ),
    index(
        accessor = building_by_planet_slot,
        btree(columns = [star_x, star_y, planet_index, slot_index])
    ),
    index(accessor = building_by_owner, btree(columns = [owner])),
    index(accessor = building_by_star, btree(columns = [star_x, star_y]))
)]
pub struct Building {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub star_x: i32,
    pub star_y: i32,
    pub planet_index: u8,
    pub slot_index: u8,
    pub kind: BuildingKind,
    pub level: u32,
    pub degradation_percent: f32,
    /// Which vein this depot targets; `f64` reserved for future rate/amount semantics.
    pub mining_material: Option<Material>,
    /// [`Some`] for **SalesDepot** (owner empire) and **MilitaryGarrison** (operator). **`None`** for unowned kinds (miner, warehouse, ship depot).
    pub owner: Option<Identity>,
    /// **MilitaryGarrison** only: combat posture. Must be `None` for other kinds.
    pub attack_mode: Option<ShipAttackMode>,
    // **MilitaryGarrison** only as well. Used in fight calculations.
    // Garrisons also have attack, defense, and warp-speed so they can be treated as ships but this
    // is determined via a lookup-table later when fights results are being calculated.
    pub health: u32,
}

#[spacetimedb::table(
    accessor = player_presence,
    index(
        accessor = presence_by_star_and_empire,
        btree(columns = [star_x, star_y, empire_id])
    ),
    index(
        accessor = presence_by_empire,
        btree(columns = [empire_id])
    )
)]
pub struct PlayerPresence {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub star_x: i32,
    pub star_y: i32,
    pub empire_id: Identity,
    pub planet_generator_key: u64,
}

#[spacetimedb::view(accessor = my_ships, public)]
pub fn my_ships(ctx: &ViewContext) -> Vec<Ship> {
    ctx.db.ship().ship_by_owner().filter(ctx.sender()).collect()
}

#[spacetimedb::view(accessor = visible_buildings, public)]
pub fn visible_buildings(ctx: &ViewContext) -> Vec<Building> {
    let sender = ctx.sender();
    ctx.db
        .player_presence()
        .presence_by_empire()
        .filter(sender)
        .flat_map(|p| {
            ctx.db
                .building()
                .building_by_star()
                .filter((p.star_x, p.star_y)) // index on (star_x, star_y)
        })
        .collect()
}

#[spacetimedb::view(accessor = visible_star_system_stock, public)]
pub fn visible_star_system_stock(ctx: &ViewContext) -> Vec<StarSystemStock> {
    let sender = ctx.sender();
    ctx.db
        .player_presence()
        .presence_by_empire()
        .filter(sender)
        .flat_map(|p| {
            ctx.db
                .star_system_stock()
                .stock_by_location()
                .filter((p.star_x, p.star_y)) // index on (star_x, star_y)
        })
        .collect()
}

/// Shared star-system warehouse state: settled kt per material, lazy mining via [`last_settled_at`].
#[spacetimedb::table(accessor = star_system_stock,
    index(accessor = stock_by_location, btree(columns = [star_x, star_y])),
    )]
pub struct StarSystemStock {
    #[primary_key]
    pub star_location_id: u128,
    pub star_x: i32,
    pub star_y: i32,
    pub last_settled_at: Timestamp,
    /// Cached ceiling from warehouses; updated on settlement.
    pub capacity_kt: f64,
    /// Merged kt per [`Material`] kind (warehouse hold).
    pub settled: Vec<Material>,
}

#[spacetimedb::table(
    accessor = ship,
    index(accessor = ship_by_owner, btree(columns = [owner])),
    index(
        accessor = ship_by_docked_star,
        btree(columns = [in_transit, star_x, star_y])
    )
)]
pub struct Ship {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub owner: Identity,
    pub stats: ShipStats,
    /// Hold contents; `f64` on each [`Material`] variant is quantity in kt.
    /// Total load must stay within [`ShipStats::size_kt`] when reducers enforce logistics.
    pub cargo: Vec<Material>,
    pub attack_mode: ShipAttackMode,

    /// `false` = docked at [`star_x`]/[`star_y`]. `true` = in warp; [`star_x`]/[`star_y`] duplicate **destination** for indexed queries.
    pub in_transit: bool,
    /// Docked: current cell. In transit: destination (same as [`transit_to_x`]/[`transit_to_y`]).
    pub star_x: i32,
    pub star_y: i32,
    /// Meaningful when [`in_transit`]; otherwise zero.
    pub transit_from_x: i32,
    pub transit_from_y: i32,
    pub transit_to_x: i32,
    pub transit_to_y: i32,
    pub transit_depart_at: Timestamp,
    pub transit_arrive_at: Timestamp,

    /// Earliest time this ship may initiate a warp while docked. Ignored while in transit.
    pub jump_ready_at: Timestamp,
    // Max and default health = ship weight in kt.
    pub health: u32,
}

/// One-shot timer to complete an in-flight warp at `arrive_at`.
#[spacetimedb::table(
    accessor = warp_job,
    scheduled(crate::ship_reducers::complete_warp)
)]
pub struct WarpJob {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub ship_id: u64,
    pub to_star_x: i32,
    pub to_star_y: i32,
}
