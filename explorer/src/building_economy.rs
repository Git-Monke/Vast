//! Mirror of [`vast::building_rules`](../../../spacetimedb/src/building_rules.rs) for UI cost previews — keep in sync.

use vast_bindings::BuildingKind;

pub const MAX_BUILDING_LEVEL: usize = 12;

pub const MIN_SHIP_KT_FOR_LEVEL: [u32; MAX_BUILDING_LEVEL] = [
    1, 5, 10, 20, 50, 100, 150, 200, 250, 300, 400, 500,
];

/// Keep in sync with [`vast::building_rules::min_ship_kt_for_level`]. `level` is 1..=MAX (UI slider).
#[inline]
#[must_use]
pub fn min_ship_kt_for_level(level: u32) -> u32 {
    MIN_SHIP_KT_FOR_LEVEL[(level - 1) as usize]
}

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

#[must_use]
pub fn sales_depot_next_cost(existing_sales_depot_count: u32) -> u64 {
    let n = u32::min(existing_sales_depot_count, 62);
    1000u64.saturating_mul(1u64 << n)
}
