//! Hardcoded building economy and ship-size gates (no closed-form level formula).

use crate::BuildingKind;
use universe::Material;

/// Maximum building level supported by [`MIN_SHIP_KT_FOR_LEVEL`] and credit tables.
pub const MAX_BUILDING_LEVEL: usize = 12;

/// Minimum ship `size_kt` by **level** (level 1 → `MIN_SHIP_KT_FOR_LEVEL[0]`, minimum 1 kt).
pub const MIN_SHIP_KT_FOR_LEVEL: [u32; MAX_BUILDING_LEVEL] = [
    1, 5, 10, 20, 50, 100, 150, 200, 250, 300, 400, 500,
];

/// `level` must be in `1..=MAX_BUILDING_LEVEL` (validated by reducers before calling).
#[inline]
#[must_use]
pub fn min_ship_kt_for_level(level: u32) -> u32 {
    MIN_SHIP_KT_FOR_LEVEL[(level - 1) as usize]
}

/// Full credit cost to **place** a leveled building at `level` (not SalesDepot).
#[must_use]
pub fn credits_for_leveled_place(kind: BuildingKind, level: u32) -> Option<u64> {
    let l = level as usize;
    if l < 1 || l > MAX_BUILDING_LEVEL {
        return None;
    }
    let base: u64 = match kind {
        BuildingKind::MiningDepot => 250,
        BuildingKind::Warehouse => 200,
        BuildingKind::MilitaryGarrison => 600,
        BuildingKind::ShipDepot => 300,
        BuildingKind::SalesDepot => return None,
    };
    let lv = l as u64;
    Some(base.saturating_mul(lv.saturating_mul(lv)))
}

/// Additional credits to charge when upgrading from `old_level` to `new_level`.
#[must_use]
pub fn credits_delta_upgrade(kind: BuildingKind, old_level: u32, new_level: u32) -> Option<u64> {
    if new_level <= old_level {
        return None;
    }
    let full_old = credits_for_leveled_place(kind, old_level)?;
    let full_new = credits_for_leveled_place(kind, new_level)?;
    Some(full_new.saturating_sub(full_old))
}

/// Next Sales Depot cost: `1000 * 2^n` where `n` = number of Sales Depots already owned.
#[must_use]
pub fn sales_depot_next_cost(existing_sales_depot_count: u32) -> u64 {
    let n = u32::min(existing_sales_depot_count, 62);
    1000u64.saturating_mul(1u64 << n)
}

/// Warehouse: kt capacity contributed by one building at `level` (`level × 1` kt).
#[inline]
#[must_use]
pub fn warehouse_kt_capacity(level: u32) -> f64 {
    f64::from(level)
}

/// Mining depot: kt/s = `level × planet_richness × resource_richness × 0.01`, scaled by degradation.
#[must_use]
pub fn mining_depot_rate_kt_s(
    level: u32,
    planet_richness: f64,
    material: &Material,
    degradation_percent: f32,
) -> f64 {
    let deg = (1.0 - (degradation_percent.clamp(0.0, 100.0) as f64 / 100.0)).max(0.0);
    f64::from(level) * planet_richness * material.multiplier() * 0.01 * deg
}

/// Garrison military power (abstract units).
#[allow(dead_code)] // used by explorer mirror; server combat reducers TBD
#[must_use]
pub fn garrison_power_units(level: u32, degradation_percent: f32) -> f64 {
    let deg = (1.0 - (degradation_percent.clamp(0.0, 100.0) as f64 / 100.0)).max(0.0);
    f64::from(level) * 100.0 * deg
}

/// Concurrent ship build slots for one ship depot.
#[allow(dead_code)] // used by explorer mirror; server spawn reducer TBD
#[inline]
#[must_use]
pub fn ship_depot_concurrent_slots(level: u32) -> u32 {
    level
}
