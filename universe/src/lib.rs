pub mod checker;
pub mod generator;
pub mod hasher;
pub mod material_stock;
pub mod resources;
pub mod settings;
pub mod ships;
pub mod star_id;

pub use material_stock::{
    accrue_settled, clamp_settled_to_capacity, get_amount, material_from_kind_kt, merge_add_kt,
    merge_into_cargo, mining_rates_hash_from_pairs, normalize_material_vec, theoretical_materials_after_accrual,
    total_kt, total_rate_kt_s, try_subtract_materials,
};
pub use resources::{Material, MaterialKind};
pub use ships::{battery_charge_duration_secs, travel_duration_secs, ShipAttackMode, ShipStats};
pub use star_id::{parse_star_id, star_display_id, star_location_id};

#[cfg(feature = "spacetimedb")]
pub use ships::{ShipAtStar, ShipInTransit, ShipLocation};
