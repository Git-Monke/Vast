mod battle;
mod building_rules;
mod buildling_settings;
mod star_economy;

use std::collections::HashMap;

use building_rules::{
    credits_delta_upgrade, credits_for_leveled_place, min_ship_kt_for_level, sales_depot_next_cost,
};
use buildling_settings::MAX_BUILDING_LEVEL;
use star_economy::{cargo_total_kt, settle_star_resources};

use spacetimedb::{
    Identity, ReducerContext, ScheduleAt, SpacetimeType, Table, TimeDuration, Timestamp,
};
use universe::generator::{StarType, generate_star, star_info_at};
use universe::material_stock::{
    get_amount, material_from_kind_kt, merge_into_cargo, normalize_material_vec, total_kt,
    try_subtract_materials,
};
use universe::settings::{COORD_UNITS_PER_LY, distance_between_cells_ly};
use universe::{Material, MaterialKind, ShipAttackMode, ShipStats, credits_for_materials_sale};

use crate::battle::{Combatant, CombatantId, CombatantResult, run_battle};

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

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
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
    index(accessor = building_by_owner, btree(columns = [owner]))
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
    /// [`Some`] for **SalesDepot** (owner empire) and **MilitaryGarrison** (operator). **`None`** for unowned kinds (miner, warehouse, ship depot).
    owner: Option<Identity>,
    /// **MilitaryGarrison** only: combat posture. Must be `None` for other kinds.
    attack_mode: Option<ShipAttackMode>,
    // **MilitaryGarrison** only as well. Used in fight calculations.
    // Garrisons also have attack, defense, and warp-speed so they can be treated as ships but this
    // is determined via a lookup-table later when fights results are being calculated.
    health: u32,
}

/// Shared star-system warehouse state: settled kt per material, lazy mining via [`last_settled_at`].
#[spacetimedb::table(accessor = star_system_stock, public)]
pub struct StarSystemStock {
    #[primary_key]
    star_location_id: u128,
    star_x: i32,
    star_y: i32,
    last_settled_at: Timestamp,
    /// Cached ceiling from warehouses; updated on settlement.
    capacity_kt: f64,
    /// Merged kt per [`Material`] kind (warehouse hold).
    settled: Vec<Material>,
}

#[spacetimedb::table(
    accessor = ship,
    public,
    index(accessor = ship_by_owner, btree(columns = [owner])),
    index(
        accessor = ship_by_docked_star,
        btree(columns = [in_transit, star_x, star_y])
    )
)]
pub struct Ship {
    #[primary_key]
    #[auto_inc]
    id: u64,
    owner: Identity,
    stats: ShipStats,
    /// Hold contents; `f64` on each [`Material`] variant is quantity in kt.
    /// Total load must stay within [`ShipStats::size_kt`] when reducers enforce logistics.
    cargo: Vec<Material>,
    attack_mode: ShipAttackMode,

    /// `false` = docked at [`star_x`]/[`star_y`]. `true` = in warp; [`star_x`]/[`star_y`] duplicate **destination** for indexed queries.
    in_transit: bool,
    /// Docked: current cell. In transit: destination (same as [`transit_to_x`]/[`transit_to_y`]).
    star_x: i32,
    star_y: i32,
    /// Meaningful when [`in_transit`]; otherwise zero.
    transit_from_x: i32,
    transit_from_y: i32,
    transit_to_x: i32,
    transit_to_y: i32,
    transit_depart_at: Timestamp,
    transit_arrive_at: Timestamp,

    /// Earliest time this ship may initiate a warp while docked. Ignored while in transit.
    jump_ready_at: Timestamp,
    // Max and default health = ship weight in kt.
    health: u32,
}

/// One-shot timer to complete an in-flight warp at `arrive_at`.
#[spacetimedb::table(accessor = warp_job, scheduled(complete_warp))]
pub struct WarpJob {
    #[primary_key]
    #[auto_inc]
    scheduled_id: u64,
    scheduled_at: ScheduleAt,
    ship_id: u64,
    to_star_x: i32,
    to_star_y: i32,
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

    if ctx.db.empire().name().find(&trimmed.to_string()).is_some() {
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

#[inline]
fn ship_docked_at_star(s: &Ship, star_x: i32, star_y: i32) -> bool {
    !s.in_transit && s.star_x == star_x && s.star_y == star_y
}

fn planet_has_enemy_garrison(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
    sender: Identity,
) -> bool {
    ctx.db.building().iter().any(|b| {
        b.star_x == star_x
            && b.star_y == star_y
            && b.planet_index == planet_index
            && b.kind == BuildingKind::MilitaryGarrison
            && b.owner != Some(sender)
    })
}

/// Any enemy military garrison anywhere at this star cell.
fn star_has_enemy_garrison(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    sender: Identity,
) -> bool {
    ctx.db.building().iter().any(|b| {
        b.star_x == star_x
            && b.star_y == star_y
            && b.kind == BuildingKind::MilitaryGarrison
            && b.owner.is_some()
            && b.owner != Some(sender)
    })
}

fn docked_ships_at_star(ctx: &ReducerContext, star_x: i32, star_y: i32) -> Vec<Ship> {
    ctx.db
        .ship()
        .ship_by_docked_star()
        .filter((false, star_x, star_y))
        .collect::<Vec<_>>()
}

fn apply_battle_results(ctx: &ReducerContext, battle_results: Vec<CombatantResult>) {
    for result in battle_results {
        match result.id {
            CombatantId::Ship(id) => {
                if let Some(ship) = ctx.db.ship().id().find(&id) {
                    if result.damage_taken >= ship.health {
                        ctx.db.ship().id().delete(&id);
                    } else {
                        ctx.db.ship().id().update(Ship {
                            health: ship.health - result.damage_taken,
                            ..ship
                        });
                    }
                }
            }
            CombatantId::Garrison(id) => {
                if let Some(building) = ctx.db.building().id().find(&id) {
                    if result.damage_taken >= building.health {
                        ctx.db.building().id().delete(&id);
                    } else {
                        ctx.db.building().id().update(Building {
                            health: building.health - result.damage_taken,
                            ..building
                        });
                    }
                }
            }
        }
    }
}

/// Aggressor vs everyone else at the star: all docked ships and all garrisons participate.
/// No-op if there is nothing to fight. Errors if the aggressor has no ships or garrisons there while enemies exist.
fn resolve_battle_at_star(
    ctx: &ReducerContext,
    aggressor: Identity,
    star_x: i32,
    star_y: i32,
) -> Result<(), String> {
    let ships = docked_ships_at_star(ctx, star_x, star_y);
    let garrisons: Vec<Building> = ctx
        .db
        .building()
        .iter()
        .filter(|b| {
            b.star_x == star_x
                && b.star_y == star_y
                && b.kind == BuildingKind::MilitaryGarrison
        })
        .collect();

    let my_ships: Vec<&Ship> = ships.iter().filter(|s| s.owner == aggressor).collect();
    let enemy_ships: Vec<&Ship> = ships.iter().filter(|s| s.owner != aggressor).collect();
    let my_garrisons: Vec<&Building> = garrisons
        .iter()
        .filter(|b| b.owner == Some(aggressor))
        .collect();
    let enemy_garrisons: Vec<&Building> = garrisons
        .iter()
        .filter(|b| b.owner != Some(aggressor))
        .collect();

    let enemy_present = !enemy_ships.is_empty() || !enemy_garrisons.is_empty();
    if !enemy_present {
        return Ok(());
    }

    if my_ships.is_empty() && my_garrisons.is_empty() {
        return Err("No forces on your side at this star".to_string());
    }

    let attackers: Vec<&dyn Combatant> = my_ships
        .iter()
        .copied()
        .map(|s| s as &dyn Combatant)
        .chain(my_garrisons.iter().map(|g| *g as &dyn Combatant))
        .collect();
    let defenders: Vec<&dyn Combatant> = enemy_ships
        .iter()
        .copied()
        .map(|s| s as &dyn Combatant)
        .chain(enemy_garrisons.iter().map(|g| *g as &dyn Combatant))
        .collect();

    let battle_results = run_battle(&attackers, &defenders);
    apply_battle_results(ctx, battle_results);
    Ok(())
}

fn player_has_stationed_ship_at_star(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    sender: Identity,
) -> bool {
    ctx.db
        .ship()
        .iter()
        .any(|s| s.owner == sender && ship_docked_at_star(&s, star_x, star_y))
}

fn max_ship_size_kt_at_star(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    sender: Identity,
) -> u32 {
    ctx.db
        .ship()
        .iter()
        .filter(|s| s.owner == sender && ship_docked_at_star(s, star_x, star_y))
        .map(|s| s.stats.size_kt)
        .max()
        .unwrap_or(0)
}

fn count_sales_depots_owned(ctx: &ReducerContext, owner: Identity) -> u32 {
    ctx.db
        .building()
        .iter()
        .filter(|b| b.kind == BuildingKind::SalesDepot && b.owner == Some(owner))
        .count() as u32
}

fn slot_occupied(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
    slot_index: u8,
) -> bool {
    ctx.db.building().iter().any(|b| {
        b.star_x == star_x
            && b.star_y == star_y
            && b.planet_index == planet_index
            && b.slot_index == slot_index
    })
}

fn deduct_credits(ctx: &ReducerContext, amount: u64) -> Result<(), String> {
    let emp = ctx
        .db
        .empire()
        .identity()
        .find(ctx.sender())
        .ok_or_else(|| "Register an empire first".to_string())?;
    if emp.credits < amount {
        return Err(format!(
            "Insufficient credits (need {}, have {})",
            amount, emp.credits
        ));
    }
    ctx.db.empire().identity().update(Empire {
        credits: emp.credits - amount,
        ..emp
    });
    Ok(())
}

fn add_credits(ctx: &ReducerContext, delta: u64) -> Result<(), String> {
    if delta == 0 {
        return Ok(());
    }
    let emp = ctx
        .db
        .empire()
        .identity()
        .find(ctx.sender())
        .ok_or_else(|| "Register an empire first".to_string())?;
    ctx.db.empire().identity().update(Empire {
        credits: emp.credits.saturating_add(delta),
        ..emp
    });
    Ok(())
}

#[must_use]
fn star_has_sales_depot(ctx: &ReducerContext, star_x: i32, star_y: i32) -> bool {
    ctx.db
        .building()
        .iter()
        .any(|b| b.star_x == star_x && b.star_y == star_y && b.kind == BuildingKind::SalesDepot)
}

/// Empty `requested`, or all-zero amounts, means sell everything in `available`.
fn resolve_sale_amounts(
    available: &[Material],
    requested: &[Material],
) -> Result<Vec<Material>, String> {
    if requested.is_empty() || total_kt(requested) <= 1e-12 {
        let mut v = available.to_vec();
        normalize_material_vec(&mut v);
        return Ok(v);
    }
    for m in requested {
        if m.amount() < 0.0 {
            return Err("Sale amounts must be non-negative".to_string());
        }
    }
    let mut need: HashMap<MaterialKind, f64> = HashMap::new();
    for m in requested {
        let q = m.amount();
        if q > 0.0 {
            *need.entry(m.kind()).or_insert(0.0) += q;
        }
    }
    let mut out = Vec::new();
    for &k in MaterialKind::ALL {
        let q = need.get(&k).copied().unwrap_or(0.0);
        if q <= 1e-12 {
            continue;
        }
        if get_amount(available, k) + 1e-9 < q {
            return Err("Not enough resources for this sale".to_string());
        }
        out.push(material_from_kind_kt(k, q));
    }
    normalize_material_vec(&mut out);
    Ok(out)
}

fn planet_has_any_building(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
) -> bool {
    ctx.db
        .building()
        .iter()
        .any(|b| b.star_x == star_x && b.star_y == star_y && b.planet_index == planet_index)
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

/// When docked: `jump_ready_at` after battery recharge at this star's temperature.
fn jump_ready_after_charge_at_star(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    stats: &ShipStats,
) -> Option<Timestamp> {
    let (st, _) = star_info_at(star_x, star_y)?;
    let secs = universe::ships::battery_charge_duration_secs(
        stats.size_kt,
        stats.battery_ly,
        st.temperature_k(),
    );
    let micros = (secs * 1_000_000.0).round() as i64;
    Some(ctx.timestamp + TimeDuration::from_micros(micros))
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

    let stats = ShipStats::default();
    let health = stats.size_kt;
    // Starter ship may warp immediately (no initial battery charge wait at spawn).
    let jump_ready_at = ctx.timestamp;

    ctx.db.ship().insert(Ship {
        id: 0,
        owner: ctx.sender(),
        stats,
        cargo: vec![],
        attack_mode: ShipAttackMode::Defend,
        in_transit: false,
        star_x,
        star_y,
        transit_from_x: 0,
        transit_from_y: 0,
        transit_to_x: 0,
        transit_to_y: 0,
        transit_depart_at: Timestamp::UNIX_EPOCH,
        transit_arrive_at: Timestamp::UNIX_EPOCH,
        jump_ready_at,
        health,
    });

    Ok(())
}

#[spacetimedb::reducer]
pub fn order_warp(
    ctx: &ReducerContext,
    ship_id: u64,
    dest_star_x: i32,
    dest_star_y: i32,
) -> Result<(), String> {
    let ship = ctx
        .db
        .ship()
        .id()
        .find(&ship_id)
        .ok_or_else(|| "Ship not found".to_string())?;
    if ship.owner != ctx.sender() {
        return Err("Not your ship".to_string());
    }

    if ship.in_transit {
        return Err("Ship is not docked at a star".to_string());
    }

    let from_x = ship.star_x;
    let from_y = ship.star_y;

    if ctx.timestamp < ship.jump_ready_at {
        return Err("Jump battery still charging".to_string());
    }

    if star_info_at(dest_star_x, dest_star_y).is_none() {
        return Err("No star at destination".to_string());
    }

    if from_x == dest_star_x && from_y == dest_star_y {
        return Err("Already at destination".to_string());
    }

    let dist_ly = distance_between_cells_ly(from_x, from_y, dest_star_x, dest_star_y);
    if dist_ly > ship.stats.battery_ly as f64 {
        return Err(format!(
            "Destination is {:.2} ly away; battery range is {} ly",
            dist_ly, ship.stats.battery_ly
        ));
    }

    let travel_secs = universe::ships::travel_duration_secs(dist_ly, ship.stats.speed_lys);
    let travel_micros = (travel_secs * 1_000_000.0).round() as i64;
    let depart = ctx.timestamp;
    let arrive = depart + TimeDuration::from_micros(travel_micros);

    ctx.db.warp_job().insert(WarpJob {
        scheduled_id: 0,
        scheduled_at: arrive.into(),
        ship_id,
        to_star_x: dest_star_x,
        to_star_y: dest_star_y,
    });

    ctx.db.ship().id().update(Ship {
        in_transit: true,
        star_x: dest_star_x,
        star_y: dest_star_y,
        transit_from_x: from_x,
        transit_from_y: from_y,
        transit_to_x: dest_star_x,
        transit_to_y: dest_star_y,
        transit_depart_at: depart,
        transit_arrive_at: arrive,
        jump_ready_at: Timestamp::UNIX_EPOCH,
        ..ship
    });

    Ok(())
}

#[spacetimedb::reducer]
pub fn complete_warp(ctx: &ReducerContext, job: WarpJob) -> Result<(), String> {
    let Some(ship) = ctx.db.ship().id().find(&job.ship_id) else {
        return Ok(());
    };

    if !ship.in_transit {
        return Ok(());
    }

    if ship.transit_to_x != job.to_star_x || ship.transit_to_y != job.to_star_y {
        return Ok(());
    }

    let Some(jump_ready_at) =
        jump_ready_after_charge_at_star(ctx, job.to_star_x, job.to_star_y, &ship.stats)
    else {
        return Ok(());
    };

    let strike_first_arrival = ship.attack_mode == ShipAttackMode::StrikeFirst;
    let aggressor = ship.owner;

    ctx.db.ship().id().update(Ship {
        in_transit: false,
        star_x: job.to_star_x,
        star_y: job.to_star_y,
        transit_from_x: 0,
        transit_from_y: 0,
        transit_to_x: 0,
        transit_to_y: 0,
        transit_depart_at: Timestamp::UNIX_EPOCH,
        transit_arrive_at: Timestamp::UNIX_EPOCH,
        jump_ready_at,
        ..ship
    });

    if strike_first_arrival {
        let others = docked_ships_at_star(ctx, job.to_star_x, job.to_star_y)
            .into_iter()
            .any(|s| s.owner != aggressor);
        if others {
            resolve_battle_at_star(ctx, aggressor, job.to_star_x, job.to_star_y)?;
        }
    }

    Ok(())
}

#[spacetimedb::reducer]
pub fn execute_battle(ctx: &ReducerContext, star_x: u32, star_y: u32) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let sx = star_x as i32;
    let sy = star_y as i32;
    resolve_battle_at_star(ctx, ctx.sender(), sx, sy)
}

/// Set a ship’s attack posture while docked. Switching to **Strike first** when other forces are at the star resolves combat only (call again afterward for other actions).
#[spacetimedb::reducer]
pub fn set_ship_attack_mode(
    ctx: &ReducerContext,
    ship_id: u64,
    attack_mode: ShipAttackMode,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let ship = ctx
        .db
        .ship()
        .id()
        .find(&ship_id)
        .ok_or_else(|| "Ship not found".to_string())?;
    if ship.owner != ctx.sender() {
        return Err("Not your ship".to_string());
    }
    if ship.in_transit {
        return Err("Ship is not docked at a star".to_string());
    }

    let sx = ship.star_x;
    let sy = ship.star_y;
    let aggressor = ship.owner;
    let want_strike = attack_mode == ShipAttackMode::StrikeFirst;

    ctx.db.ship().id().update(Ship {
        attack_mode,
        ..ship
    });

    if want_strike {
        let others = docked_ships_at_star(ctx, sx, sy)
            .into_iter()
            .any(|s| s.owner != aggressor);
        if others {
            resolve_battle_at_star(ctx, aggressor, sx, sy)?;
        }
    }

    Ok(())
}

/// Load resources from the star’s shared warehouse onto a docked ship. Runs settlement first.
#[spacetimedb::reducer]
pub fn collect_star_resources(
    ctx: &ReducerContext,
    ship_id: u64,
    pickup: Vec<Material>,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }
    for m in &pickup {
        let q = match m {
            Material::Iron(q) | Material::Helium(q) => *q,
        };
        if q < 0.0 {
            return Err("Pickup amounts must be non-negative".to_string());
        }
    }
    if total_kt(&pickup) <= 1e-12 {
        return Err("Specify a positive amount to collect".to_string());
    }

    let ship = ctx
        .db
        .ship()
        .id()
        .find(&ship_id)
        .ok_or_else(|| "Ship not found".to_string())?;
    if ship.owner != ctx.sender() {
        return Err("Not your ship".to_string());
    }

    if ship.in_transit {
        return Err("Ship is not docked at a star".to_string());
    }
    let star_x = ship.star_x;
    let star_y = ship.star_y;

    if star_has_enemy_garrison(ctx, star_x, star_y, ctx.sender()) {
        resolve_battle_at_star(ctx, ctx.sender(), star_x, star_y)?;
        return Ok(());
    }

    settle_star_resources(ctx, star_x, star_y)?;

    let sid = star_location_id(star_x, star_y);
    let row = ctx
        .db
        .star_system_stock()
        .star_location_id()
        .find(&sid)
        .ok_or_else(|| "Star system stock missing".to_string())?;

    let new_cargo_total = cargo_total_kt(&ship.cargo) + total_kt(&pickup);
    if new_cargo_total > ship.stats.size_kt as f64 + 1e-6 {
        return Err(format!(
            "Cargo capacity exceeded (would be {:.2} / {} kt)",
            new_cargo_total, ship.stats.size_kt
        ));
    }

    let mut settled = row.settled.clone();
    try_subtract_materials(&mut settled, &pickup)?;

    ctx.db
        .star_system_stock()
        .star_location_id()
        .update(StarSystemStock { settled, ..row });

    let mut cargo = ship.cargo.clone();
    merge_into_cargo(&mut cargo, &pickup);

    ctx.db.ship().id().update(Ship { cargo, ..ship });

    Ok(())
}

/// Sell ship cargo to the government sink at a star that has a Sales Depot. Empty `amounts` sells all.
#[spacetimedb::reducer]
pub fn sell_ship_cargo(
    ctx: &ReducerContext,
    ship_id: u64,
    amounts: Vec<Material>,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let ship = ctx
        .db
        .ship()
        .id()
        .find(&ship_id)
        .ok_or_else(|| "Ship not found".to_string())?;
    if ship.owner != ctx.sender() {
        return Err("Not your ship".to_string());
    }

    if ship.in_transit {
        return Err("Ship is not docked at a star".to_string());
    }
    let star_x = ship.star_x;
    let star_y = ship.star_y;

    if !star_has_sales_depot(ctx, star_x, star_y) {
        return Err("No Sales Depot at this star".to_string());
    }

    let to_sell = resolve_sale_amounts(&ship.cargo, &amounts)?;
    if total_kt(&to_sell) <= 1e-12 {
        return Ok(());
    }

    let credits = credits_for_materials_sale(&to_sell);
    let mut cargo = ship.cargo.clone();
    try_subtract_materials(&mut cargo, &to_sell)?;
    add_credits(ctx, credits)?;
    ctx.db.ship().id().update(Ship { cargo, ..ship });

    Ok(())
}

/// Sell from the star’s shared warehouse at baseline prices. Empty `amounts` sells all settled stock. Runs settlement first.
#[spacetimedb::reducer]
pub fn sell_star_warehouse(
    ctx: &ReducerContext,
    ship_id: u64,
    amounts: Vec<Material>,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let ship = ctx
        .db
        .ship()
        .id()
        .find(&ship_id)
        .ok_or_else(|| "Ship not found".to_string())?;
    if ship.owner != ctx.sender() {
        return Err("Not your ship".to_string());
    }

    if ship.in_transit {
        return Err("Ship is not docked at a star".to_string());
    }
    let star_x = ship.star_x;
    let star_y = ship.star_y;

    if !star_has_sales_depot(ctx, star_x, star_y) {
        return Err("No Sales Depot at this star".to_string());
    }

    if star_has_enemy_garrison(ctx, star_x, star_y, ctx.sender()) {
        resolve_battle_at_star(ctx, ctx.sender(), star_x, star_y)?;
        return Ok(());
    }

    settle_star_resources(ctx, star_x, star_y)?;

    let sid = star_location_id(star_x, star_y);
    let row = ctx
        .db
        .star_system_stock()
        .star_location_id()
        .find(&sid)
        .ok_or_else(|| "Star system stock missing".to_string())?;

    let to_sell = resolve_sale_amounts(&row.settled, &amounts)?;
    if total_kt(&to_sell) <= 1e-12 {
        return Ok(());
    }

    let credits = credits_for_materials_sale(&to_sell);
    let mut settled = row.settled.clone();
    try_subtract_materials(&mut settled, &to_sell)?;
    add_credits(ctx, credits)?;
    ctx.db
        .star_system_stock()
        .star_location_id()
        .update(StarSystemStock { settled, ..row });

    Ok(())
}

#[spacetimedb::reducer]
pub fn place_building(
    ctx: &ReducerContext,
    star_x: i32,
    star_y: i32,
    planet_index: u8,
    slot_index: u8,
    kind: BuildingKind,
    level: u32,
    mining_resource_index: Option<u8>,
    garrison_attack_mode: Option<ShipAttackMode>,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let Some(sys) = generate_star(star_x, star_y) else {
        return Err("No star system at these coordinates".to_string());
    };

    let Some(planet) = sys.planets.iter().find(|p| p.index == planet_index) else {
        return Err("Invalid planet index for this system".to_string());
    };

    if planet_has_enemy_garrison(ctx, star_x, star_y, planet_index, ctx.sender()) {
        return Err("Enemy military garrison controls this planet".to_string());
    }

    if !player_has_stationed_ship_at_star(ctx, star_x, star_y, ctx.sender()) {
        return Err("You need a ship stationed at this star (not in transit)".to_string());
    }

    if slot_index >= planet.size {
        return Err("Slot index out of range for this planet".to_string());
    }

    if slot_occupied(ctx, star_x, star_y, planet_index, slot_index) {
        return Err("That building slot is already occupied".to_string());
    }

    let max_kt = max_ship_size_kt_at_star(ctx, star_x, star_y, ctx.sender());

    let (cost, owner, eff_level, mining_mat, attack_mode, health) = match kind {
        BuildingKind::SalesDepot => {
            let n = count_sales_depots_owned(ctx, ctx.sender());
            let c = sales_depot_next_cost(n);
            (c, Some(ctx.sender()), 1_u32, None, None, 0)
        }
        BuildingKind::MiningDepot => {
            let lv = level;
            if lv < 1 || lv as usize > MAX_BUILDING_LEVEL {
                return Err(format!("Level must be 1..={MAX_BUILDING_LEVEL}"));
            }
            let need = min_ship_kt_for_level(lv);
            if max_kt < need {
                return Err(format!(
                    "Need a ship of at least {need} kt at this star (largest here: {max_kt} kt)"
                ));
            }
            let ri =
                mining_resource_index.ok_or_else(|| "Mining depot requires a resource index")?;
            let mat = planet
                .resources
                .get(ri as usize)
                .cloned()
                .ok_or_else(|| "Invalid resource index for this planet".to_string())?;
            let c = credits_for_leveled_place(kind, lv)
                .ok_or_else(|| "Invalid level for this building kind".to_string())?;
            (c, None, lv, Some(mat), None, 0)
        }
        BuildingKind::Warehouse | BuildingKind::ShipDepot => {
            let lv = level;
            if lv < 1 || lv as usize > MAX_BUILDING_LEVEL {
                return Err(format!("Level must be 1..={MAX_BUILDING_LEVEL}"));
            }
            let need = min_ship_kt_for_level(lv);
            if max_kt < need {
                return Err(format!(
                    "Need a ship of at least {need} kt at this star (largest here: {max_kt} kt)"
                ));
            }
            let c = credits_for_leveled_place(kind, lv)
                .ok_or_else(|| "Invalid level for this building kind".to_string())?;
            (c, None, lv, None, None, 0)
        }
        BuildingKind::MilitaryGarrison => {
            let lv = level;
            if lv < 1 || lv as usize > MAX_BUILDING_LEVEL {
                return Err(format!("Level must be 1..={MAX_BUILDING_LEVEL}"));
            }
            let need = min_ship_kt_for_level(lv);
            if max_kt < need {
                return Err(format!(
                    "Need a ship of at least {need} kt at this star (largest here: {max_kt} kt)"
                ));
            }
            let c = credits_for_leveled_place(kind, lv)
                .ok_or_else(|| "Invalid level for this building kind".to_string())?;
            let am = garrison_attack_mode
                .ok_or_else(|| "Military garrison requires attack mode".to_string())?;
            (c, Some(ctx.sender()), lv, None, Some(am), 1)
        }
    };

    settle_star_resources(ctx, star_x, star_y)?;

    deduct_credits(ctx, cost)?;

    ctx.db.building().insert(Building {
        id: 0,
        star_x,
        star_y,
        planet_index,
        slot_index,
        kind,
        level: eff_level,
        degradation_percent: 0.0,
        mining_material: mining_mat,
        owner,
        attack_mode,
        health,
    });

    Ok(())
}

#[spacetimedb::reducer]
pub fn upgrade_building(
    ctx: &ReducerContext,
    building_id: u64,
    new_level: u32,
) -> Result<(), String> {
    if ctx.db.empire().identity().find(ctx.sender()).is_none() {
        return Err("Register an empire first".to_string());
    }

    let b = ctx
        .db
        .building()
        .id()
        .find(&building_id)
        .ok_or_else(|| "Building not found".to_string())?;

    if b.kind == BuildingKind::SalesDepot {
        return Err("Sales depots do not have upgradeable levels".to_string());
    }

    if new_level <= b.level {
        return Err("New level must be greater than current level".to_string());
    }

    if new_level as usize > MAX_BUILDING_LEVEL {
        return Err(format!("Level must be at most {MAX_BUILDING_LEVEL}"));
    }

    if planet_has_enemy_garrison(ctx, b.star_x, b.star_y, b.planet_index, ctx.sender()) {
        return Err("Enemy military garrison controls this planet".to_string());
    }

    if !player_has_stationed_ship_at_star(ctx, b.star_x, b.star_y, ctx.sender()) {
        return Err("You need a ship stationed at this star (not in transit)".to_string());
    }

    let max_kt = max_ship_size_kt_at_star(ctx, b.star_x, b.star_y, ctx.sender());
    let need = min_ship_kt_for_level(new_level);
    if max_kt < need {
        return Err(format!(
            "Need a ship of at least {need} kt at this star (largest here: {max_kt} kt)"
        ));
    }

    if b.kind == BuildingKind::MilitaryGarrison && b.owner != Some(ctx.sender()) {
        return Err("Not your military garrison".to_string());
    }

    let delta = credits_delta_upgrade(b.kind, b.level, new_level)
        .ok_or_else(|| "Invalid upgrade".to_string())?;

    settle_star_resources(ctx, b.star_x, b.star_y)?;

    deduct_credits(ctx, delta)?;

    ctx.db.building().id().update(Building {
        level: new_level,
        ..b
    });

    Ok(())
}
