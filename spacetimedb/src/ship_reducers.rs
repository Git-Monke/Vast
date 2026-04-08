use crate::keys::generate_planet_key;
use spacetimedb::{ReducerContext, Table, TimeDuration, Timestamp};
use universe::generator::star_info_at;

use crate::{empire, ship, warp_job};
use universe::settings::distance_between_cells_ly;
use universe::{ShipAttackMode, ShipStats};

use crate::combat::resolve_battle_at_star;
use crate::db_helpers::{
    find_empty_red_dwarf_starter, jump_ready_after_charge_at_star, owner_has_any_ship,
    update_player_presence,
};
use crate::{Ship, WarpJob};

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

    update_player_presence(ctx, ctx.sender(), star_x, star_y);

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

    update_player_presence(ctx, ship.owner, from_x, from_y);

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

    update_player_presence(ctx, ship.owner, job.to_star_x, job.to_star_y);

    if strike_first_arrival {
        let others = crate::db_helpers::docked_ships_at_star(ctx, job.to_star_x, job.to_star_y)
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
        let others = crate::db_helpers::docked_ships_at_star(ctx, sx, sy)
            .into_iter()
            .any(|s| s.owner != aggressor);
        if others {
            resolve_battle_at_star(ctx, aggressor, sx, sy)?;
        }
    }

    Ok(())
}
