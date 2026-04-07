use spacetimedb::{Identity, ReducerContext, ScheduleAt, SpacetimeType};
use universe::{Material, ShipStats, generator::PlanetType};

use crate::BuildingKind;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum ScanInitiator {
    Radar,
    Ship,
}

#[derive(SpacetimeType)]
struct ScannedBuildling {
    pub kind: BuildingKind,
    pub level: u32,
    pub degradation_percent: f32,
    pub mining_material: Option<Material>,
    // Only radars, sales depots, and garrisons have owners
    pub owner: Option<Identity>,
    pub health: u32,
}

#[derive(SpacetimeType)]
struct ScannedPlanet {
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
    scheduled(complete_scan)
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

#[spacetimedb::table(accessor = scan_result)]
pub struct ScanResult {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub empire_id: Identity,
    pub star_x: i32,
    pub star_y: i32,

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
    Ok(())
}

#[spacetimedb::reducer]
pub fn complete_scan(ctx: &ReducerContext, job: ScanJob) -> Result<(), String> {
    Ok(())
}
