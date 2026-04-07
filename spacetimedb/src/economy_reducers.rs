use spacetimedb::ReducerContext;

use crate::star_economy::{cargo_total_kt, settle_star_resources};
use crate::{empire, ship, star_system_stock};
use universe::material_stock::{merge_into_cargo, total_kt, try_subtract_materials};
use universe::{Material, credits_for_materials_sale};

use crate::combat::resolve_battle_at_star;
use crate::db_helpers::{
    add_credits, resolve_sale_amounts, star_has_enemy_garrison, star_has_sales_depot,
};
use crate::{star_location_id, Ship, StarSystemStock};

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
