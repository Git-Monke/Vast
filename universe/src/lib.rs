pub mod checker;
pub mod generator;
pub mod hasher;
pub mod resources;
pub mod ships;

pub use resources::{Material, MaterialKind};
pub use ships::{travel_duration_secs, ShipAttackMode, ShipStats};

#[cfg(feature = "spacetimedb")]
pub use ships::{ShipAtPlanet, ShipInTransit, ShipLocation};
