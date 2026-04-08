use std::collections::HashMap;

use spacetimedb::{Identity, ReducerContext, Table, TimeDuration, Timestamp};
use universe::generator::{StarType, generate_star, star_info_at};
use universe::material_stock::{
    get_amount, material_from_kind_kt, normalize_material_vec, total_kt,
};
use universe::settings::COORD_UNITS_PER_LY;
use universe::{Material, MaterialKind, ShipStats};

use crate::battle::{CombatantId, CombatantResult};
use crate::constants::{
    MAX_STARTER_SAMPLE_ATTEMPTS, STARTER_DISK_RADIUS_LY, STARTER_LOCAL_GRID, STARTER_LOCAL_HALF,
};
use crate::{
    Building, BuildingKind, Empire, PlayerPresence, Ship, building, empire, player_presence, ship,
};

pub(crate) fn apply_battle_results(ctx: &ReducerContext, battle_results: Vec<CombatantResult>) {
    for result in battle_results {
        match result.id {
            CombatantId::Ship(id) => {
                if let Some(ship) = ctx.db.ship().id().find(&id) {
                    if result.damage_taken >= ship.health {
                        let owner = ship.owner;
                        let star_x = ship.star_x;
                        let star_y = ship.star_y;
                        ctx.db.ship().id().delete(&id);
                        update_player_presence(ctx, owner, star_x, star_y);
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
                        let owner = building.owner;
                        let star_x = building.star_x;
                        let star_y = building.star_y;
                        let kind = building.kind;
                        ctx.db.building().id().delete(&id);
                        if kind == BuildingKind::Radar {
                            if let Some(owner_id) = owner {
                                update_player_presence(ctx, owner_id, star_x, star_y);
                            }
                        }
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

pub(crate) fn update_player_presence(
    ctx: &ReducerContext,
    empire_id: Identity,
    star_x: i32,
    star_y: i32,
) {
    let has_ship = ctx
        .db
        .ship()
        .ship_by_docked_star()
        .filter((false, star_x, star_y))
        .any(|s| s.owner == empire_id);
    let has_radar = ctx
        .db
        .building()
        .building_by_planet_location()
        .filter((star_x, star_y))
        .any(|b| b.owner == Some(empire_id) && b.kind == BuildingKind::Radar);

    let existing = ctx
        .db
        .player_presence()
        .presence_by_star_and_empire()
        .filter((star_x, star_y, empire_id))
        .next();

    if has_ship || has_radar {
        if existing.is_none() {
            ctx.db.player_presence().insert(PlayerPresence {
                id: 0,
                star_x,
                star_y,
                empire_id,
            });
        }
    } else if let Some(row) = existing {
        ctx.db.player_presence().id().delete(&row.id);
    }
}

pub(crate) fn owner_has_any_ship(ctx: &ReducerContext, owner: Identity) -> bool {
    ctx.db.ship().iter().any(|s| s.owner == owner)
}

#[inline]
pub(crate) fn ship_docked_at_star(s: &Ship, star_x: i32, star_y: i32) -> bool {
    !s.in_transit && s.star_x == star_x && s.star_y == star_y
}

pub(crate) fn planet_has_enemy_garrison(
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
pub(crate) fn star_has_enemy_garrison(
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

pub(crate) fn docked_ships_at_star(ctx: &ReducerContext, star_x: i32, star_y: i32) -> Vec<Ship> {
    ctx.db
        .ship()
        .ship_by_docked_star()
        .filter((false, star_x, star_y))
        .collect::<Vec<_>>()
}

pub(crate) fn player_has_stationed_ship_at_star(
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

pub(crate) fn max_ship_size_kt_at_star(
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

pub(crate) fn count_sales_depots_owned(ctx: &ReducerContext, owner: Identity) -> u32 {
    ctx.db
        .building()
        .iter()
        .filter(|b| b.kind == BuildingKind::SalesDepot && b.owner == Some(owner))
        .count() as u32
}

pub(crate) fn slot_occupied(
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

pub(crate) fn deduct_credits(ctx: &ReducerContext, amount: u64) -> Result<(), String> {
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

pub(crate) fn add_credits(ctx: &ReducerContext, delta: u64) -> Result<(), String> {
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
pub(crate) fn star_has_sales_depot(ctx: &ReducerContext, star_x: i32, star_y: i32) -> bool {
    ctx.db
        .building()
        .iter()
        .any(|b| b.star_x == star_x && b.star_y == star_y && b.kind == BuildingKind::SalesDepot)
}

/// Empty `requested`, or all-zero amounts, means sell everything in `available`.
pub(crate) fn resolve_sale_amounts(
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

pub(crate) fn find_empty_red_dwarf_starter(ctx: &ReducerContext) -> Option<(i32, i32)> {
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
pub(crate) fn jump_ready_after_charge_at_star(
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
