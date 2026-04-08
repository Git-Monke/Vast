//! Star-system warehouse capacity, mining rates, military power, and lazy settlement.

use std::collections::HashMap;

use spacetimedb::{ReducerContext, Table, Timestamp};

use crate::keys::generate_planet_key;
use crate::building;
use crate::star_system_stock;
use crate::{star_location_id, Building, BuildingKind, StarSystemStock};
use universe::generator::{generate_star, StarSystem};
use universe::material_stock::{self, mining_rates_hash_from_pairs, total_kt, total_rate_kt_s};
use universe::Material;
use universe::MaterialKind;

use crate::building_rules::{mining_depot_rate_kt_s, warehouse_kt_capacity};

/// Microseconds since Unix epoch as `i128` for safe subtraction.
#[inline]
fn ts_micros(t: Timestamp) -> i128 {
    t.to_duration_since_unix_epoch()
        .map(|d| d.as_micros() as i128)
        .unwrap_or(0)
}

/// Elapsed seconds from `earlier` to `later` (may be negative if misordered — clamp to 0).
#[must_use]
pub fn elapsed_seconds(later: Timestamp, earlier: Timestamp) -> f64 {
    let d = ts_micros(later) - ts_micros(earlier);
    if d <= 0 {
        return 0.0;
    }
    d as f64 / 1_000_000.0
}

/// Sum of warehouse contributions: `level × 1` kt each.
#[must_use]
pub fn capacity_kt_at_star(buildings: &[Building]) -> f64 {
    buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::Warehouse)
        .map(|b| warehouse_kt_capacity(b.level))
        .sum()
}

/// Per-kind kt/s from all mining depots at this star.
#[must_use]
pub fn mining_rates_kt_s(sys: &StarSystem, buildings: &[Building]) -> HashMap<MaterialKind, f64> {
    let pairs = buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::MiningDepot)
        .filter_map(|b| {
            let mat = b.mining_material.as_ref()?;
            let planet = sys.planets.iter().find(|p| p.index == b.planet_index)?;
            let r = mining_depot_rate_kt_s(b.level, planet.richness, mat, b.degradation_percent);
            Some((mat.kind(), r))
        });
    mining_rates_hash_from_pairs(pairs)
}

/// Ensure a [`StarSystemStock`] row exists for this star.
pub fn ensure_star_system_stock(ctx: &ReducerContext, star_x: i32, star_y: i32) {
    let id = star_location_id(star_x, star_y);
    if ctx.db.star_system_stock().star_location_id().find(&id).is_some() {
        return;
    }
    let buildings: Vec<Building> = ctx
        .db
        .building()
        .iter()
        .filter(|b| b.star_x == star_x && b.star_y == star_y)
        .collect();
    let cap = capacity_kt_at_star(&buildings);
    ctx.db.star_system_stock().insert(StarSystemStock {
        star_location_id: id,
        star_x,
        star_y,
        last_settled_at: ctx.timestamp,
        capacity_kt: cap,
        settled: vec![],
    });
}

/// Accrue theoretical production into settled amounts; update `last_settled_at` and `capacity_kt`.
pub fn settle_star_resources(ctx: &ReducerContext, star_x: i32, star_y: i32) -> Result<(), String> {
    let planet_generator_key = generate_planet_key(star_x, star_y);
    let Some(sys) = generate_star(star_x, star_y, Some(planet_generator_key)) else {
        return Err("No star system at these coordinates".to_string());
    };

    ensure_star_system_stock(ctx, star_x, star_y);
    let id = star_location_id(star_x, star_y);

    let buildings: Vec<Building> = ctx
        .db
        .building()
        .iter()
        .filter(|b| b.star_x == star_x && b.star_y == star_y)
        .collect();

    let row = ctx
        .db
        .star_system_stock()
        .star_location_id()
        .find(&id)
        .ok_or_else(|| "Star system stock missing".to_string())?;

    let capacity_kt = capacity_kt_at_star(&buildings);
    let rates = mining_rates_kt_s(&sys, &buildings);
    let total_rate = total_rate_kt_s(&rates);

    let dt = elapsed_seconds(ctx.timestamp, row.last_settled_at);
    let mut settled = row.settled.clone();
    material_stock::normalize_material_vec(&mut settled);
    let remaining_kt = (capacity_kt - total_kt(&settled)).max(0.0);

    let t_max = if total_rate > 0.0 {
        remaining_kt / total_rate
    } else {
        f64::INFINITY
    };
    let t_eff = dt.min(t_max);

    material_stock::accrue_settled(&mut settled, &rates, t_eff, capacity_kt);

    ctx.db.star_system_stock().star_location_id().update(StarSystemStock {
        capacity_kt,
        settled,
        last_settled_at: ctx.timestamp,
        ..row
    });

    Ok(())
}

#[must_use]
pub fn cargo_total_kt(cargo: &[Material]) -> f64 {
    total_kt(cargo)
}
