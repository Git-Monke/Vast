pub mod checker;
pub mod generator;
pub mod hasher;
pub mod resources;
pub mod settings;
pub mod ships;
pub mod star_id;

pub use resources::{Material, MaterialKind};
pub use ships::{battery_charge_duration_secs, travel_duration_secs, ShipAttackMode, ShipStats};
pub use star_id::{parse_star_id, star_display_id};

#[cfg(feature = "spacetimedb")]
pub use ships::{ShipAtStar, ShipInTransit, ShipLocation};
