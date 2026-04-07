use spacetimedb::{Identity, ReducerContext, Table};

use crate::battle::{Combatant, CombatantId, CombatantResult, run_battle};
use crate::{building, ship};
use crate::db_helpers::docked_ships_at_star;
use crate::{Building, BuildingKind, Ship};

pub(crate) fn apply_battle_results(ctx: &ReducerContext, battle_results: Vec<CombatantResult>) {
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
pub(crate) fn resolve_battle_at_star(
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
