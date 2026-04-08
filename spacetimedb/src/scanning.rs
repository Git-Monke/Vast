use spacetimedb::{Identity, ReducerContext, ScheduleAt, SpacetimeType, Table, TimeDuration};
use universe::{
    Material, ShipStats,
    generator::{PlanetType, generate_star, star_info_at},
    settings::distance_between_cells_ly,
};

use crate::{BuildingKind, building, buildling_settings::RADAR_MAX_LY_FOR_LEVEL, ship, keys::generate_planet_key};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum ScanInitiator {
    Radar,
    Ship,
}

#[derive(SpacetimeType)]
pub(crate) struct ScannedBuildling {
    pub kind: BuildingKind,
    pub level: u32,
    pub degradation_percent: f32,
    pub mining_material: Option<Material>,
    // Only radars, sales depots, and garrisons have owners
    pub owner: Option<Identity>,
    pub health: u32,
}

#[derive(SpacetimeType)]
pub(crate) struct ScannedPlanet {
    pub index: u8,
    pub name: String,
    pub temperature_k: f64,
    pub planet_type: PlanetType,
    pub size: u8,      // buildable slots, 1–10
    pub richness: f64, // multiplier, e.g. 1.5× base yield
    pub resources: Vec<Material>,
    pub buildlings: Vec<ScannedBuildling>,
}

#[derive(SpacetimeType)]
pub struct ScannedDockedShip {
    pub owner: Identity,
    pub stats: ShipStats,
    pub health: u32,
}

/// One-shot timer to complete an in-flight warp at `arrive_at`.
#[spacetimedb::table(
    accessor = scan_job,
    scheduled(complete_scan),
    public
)]
pub struct ScanJob {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,

    pub empire_id: Identity,
    pub scan_initiator: ScanInitiator,
    pub initiator_id: u64,

    pub ship_id: u64,
    pub to_star_x: i32,
    pub to_star_y: i32,
}

#[spacetimedb::table(accessor = scan_result, public)]
pub struct ScanResult {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub empire_id: Identity,
    pub star_x: i32,
    pub star_y: i32,

    pub planet_generator_key: u64,
    pub planets: Vec<ScannedPlanet>,
    pub docked_ships: Vec<ScannedDockedShip>,
}

#[spacetimedb::reducer]
pub fn initiate_scan(
    ctx: &ReducerContext,
    scan_initiator: ScanInitiator,
    initiator_id: u64,
    to_star_x: i32,
    to_star_y: i32,
) -> Result<(), String> {
    let empire_id = ctx.sender();

    let (init_star_x, init_star_y, max_scan_ly) = match scan_initiator {
        ScanInitiator::Radar => {
            let b = ctx
                .db
                .building()
                .id()
                .find(&initiator_id)
                .ok_or_else(|| "Radar not found".to_string())?;
            if b.kind != BuildingKind::Radar {
                return Err("Initiator is not a radar".to_string());
            }
            if b.owner != Some(empire_id) {
                return Err("Not your radar".to_string());
            }
            let max_ly = RADAR_MAX_LY_FOR_LEVEL[(b.level - 1) as usize];
            (b.star_x, b.star_y, max_ly)
        }
        ScanInitiator::Ship => {
            let s = ctx
                .db
                .ship()
                .id()
                .find(&initiator_id)
                .ok_or_else(|| "Ship not found".to_string())?;
            if s.owner != empire_id {
                return Err("Not your ship".to_string());
            }
            if s.in_transit {
                return Err("Ship is in transit".to_string());
            }
            (s.star_x, s.star_y, s.stats.radar_ly as f64)
        }
    };

    if ctx
        .db
        .scan_job()
        .iter()
        .any(|j| j.scan_initiator == scan_initiator && j.initiator_id == initiator_id)
    {
        return Err("Initiator already has an active scan job".to_string());
    }

    if init_star_x == to_star_x && init_star_y == to_star_y {
        return Err("Already at destination".to_string());
    }

    if star_info_at(to_star_x, to_star_y).is_none() {
        return Err("No star at destination".to_string());
    }

    let dist_ly = distance_between_cells_ly(init_star_x, init_star_y, to_star_x, to_star_y);
    if dist_ly > max_scan_ly {
        return Err(format!(
            "Target is {:.2} ly away; max scan range is {:.2} ly",
            dist_ly, max_scan_ly
        ));
    }

    let secs = (dist_ly / 10.0).ceil() as i64;
    let scheduled_at = (ctx.timestamp + TimeDuration::from_micros(secs * 1_000_000)).into();

    let ship_id = if matches!(scan_initiator, ScanInitiator::Ship) {
        initiator_id
    } else {
        0
    };

    ctx.db.scan_job().insert(ScanJob {
        scheduled_id: 0,
        scheduled_at,
        empire_id,
        scan_initiator,
        initiator_id,
        ship_id,
        to_star_x,
        to_star_y,
    });

    Ok(())
}

#[spacetimedb::reducer]
pub fn complete_scan(ctx: &ReducerContext, job: ScanJob) -> Result<(), String> {
    // Delete existing scan results for this empire and location so we only keep the latest.
    let old_results: Vec<u64> = ctx
        .db
        .scan_result()
        .iter()
        .filter(|r| {
            r.empire_id == job.empire_id && r.star_x == job.to_star_x && r.star_y == job.to_star_y
        })
        .map(|r| r.id)
        .collect();

    for id in old_results {
        ctx.db.scan_result().id().delete(&id);
    }

    let planet_generator_key = generate_planet_key(job.to_star_x, job.to_star_y);

    let sys = generate_star(job.to_star_x, job.to_star_y, Some(planet_generator_key))
        .ok_or_else(|| "No star system at destination".to_string())?;

    let planets = sys
        .planets
        .iter()
        .map(|p| {
            let buildlings = ctx
                .db
                .building()
                .building_by_planet_location()
                .filter((job.to_star_x, job.to_star_y, p.index))
                .map(|b| ScannedBuildling {
                    kind: b.kind,
                    level: b.level,
                    degradation_percent: b.degradation_percent,
                    mining_material: b.mining_material.clone(),
                    owner: b.owner,
                    health: b.health,
                })
                .collect();

            ScannedPlanet {
                index: p.index,
                name: p.name.clone(),
                temperature_k: p.temperature_k,
                planet_type: p.planet_type,
                size: p.size,
                richness: p.richness,
                resources: p.resources.clone(),
                buildlings,
            }
        })
        .collect();

    let docked_ships = ctx
        .db
        .ship()
        .ship_by_docked_star()
        .filter((false, job.to_star_x, job.to_star_y))
        .map(|s| ScannedDockedShip {
            owner: s.owner,
            stats: s.stats,
            health: s.health,
        })
        .collect();

    ctx.db.scan_result().insert(ScanResult {
        id: 0,
        empire_id: job.empire_id,
        star_x: job.to_star_x,
        star_y: job.to_star_y,
        planet_generator_key,
        planets,
        docked_ships,
    });

    Ok(())
}
