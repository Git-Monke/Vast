use spacetimedb::{ReducerContext, Table};

use crate::{building, empire};
use crate::building_rules::{
    credits_delta_upgrade, credits_for_leveled_place, min_ship_kt_for_level, sales_depot_next_cost,
};
use crate::buildling_settings::MAX_BUILDING_LEVEL;
use crate::star_economy::settle_star_resources;
use universe::generator::generate_star;

use crate::db_helpers::{
    count_sales_depots_owned, deduct_credits, max_ship_size_kt_at_star,
    planet_has_enemy_garrison, player_has_stationed_ship_at_star, slot_occupied,
};
use crate::{Building, BuildingKind};
use universe::ShipAttackMode;

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
