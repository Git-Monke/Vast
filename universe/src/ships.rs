/// Travel time in seconds for a straight-line jump: `distance_ly / speed_lys`.
#[inline]
#[must_use]
pub fn travel_duration_secs(distance_ly: f64, speed_lys: f64) -> f64 {
    if speed_lys <= 0.0 {
        return f64::INFINITY;
    }
    distance_ly / speed_lys
}

#[cfg_attr(feature = "spacetimedb", derive(spacetimedb::SpacetimeType))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShipAttackMode {
    Defend,
    StrikeFirst,
}

/// In a star system (grid coords; same convention as stars). Ships are not pinned to a planet.
#[cfg(feature = "spacetimedb")]
#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct ShipAtStar {
    pub star_x: i32,
    pub star_y: i32,
}

/// En route between two star cells; timestamps from the server when reducers run.
#[cfg(feature = "spacetimedb")]
#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct ShipInTransit {
    pub from_star_x: i32,
    pub from_star_y: i32,
    pub to_star_x: i32,
    pub to_star_y: i32,
    pub depart_at: spacetimedb::Timestamp,
    pub arrive_at: spacetimedb::Timestamp,
}

#[cfg(feature = "spacetimedb")]
#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub enum ShipLocation {
    AtStar(ShipAtStar),
    InTransit(ShipInTransit),
}

#[cfg_attr(feature = "spacetimedb", derive(spacetimedb::SpacetimeType))]
#[derive(Clone, Debug, PartialEq)]
pub struct ShipStats {
    pub size_kt: u32,    // cargo capacity in kilotonnes
    pub speed_lys: f64, // travel speed in light-years per second
    pub defense: u32,           // hit points required to destroy
    pub attack: u32,            // damage per volley
    pub battery_ly: u32,        // jump distance before recharge
    pub radar_ly: u32,          // scanning range in light-years
}

impl Default for ShipStats {
    fn default() -> Self {
        Self {
            size_kt: 10,
            speed_lys: 0.1,
            defense: 10,
            attack: 0,
            battery_ly: 10,
            radar_ly: 5,
        }
    }
}

impl ShipStats {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.speed_lys < 0.1 {
            return Err("Speed must be at least 0.1 ly/s");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct CostBreakdown {
    pub size_dev_credits: u64,
    pub speed_dev_credits: u64,
    pub attack_dev_credits: u64,
    pub defense_dev_credits: u64,
    pub battery_dev_credits: u64,
    pub radar_dev_credits: u64,
    pub total_dev_credits: u64,

    pub size_dev_minutes: u64,
    pub speed_dev_minutes: u64,
    pub attack_dev_minutes: u64,
    pub defense_dev_minutes: u64,
    pub battery_dev_minutes: u64,
    pub radar_dev_minutes: u64,
    pub total_dev_minutes: u64,

    // Base maintenance costs (before multiplication)
    pub size_maint_base_credits: u64,
    pub speed_maint_base_credits: u64,
    pub attack_maint_base_credits: u64,
    pub defense_maint_base_credits: u64,
    pub battery_maint_base_credits: u64,
    pub radar_maint_base_credits: u64,

    // Final multiplied maintenance costs for each component
    pub size_maint_credits: u64,
    pub speed_maint_credits: u64,
    pub attack_maint_credits: u64,
    pub defense_maint_credits: u64,
    pub battery_maint_credits: u64,
    pub radar_maint_credits: u64, // radar does not multiply with other stats
    pub total_maint_credits: u64,

    // Interconnected multiplier tracking (for UI display)
    pub speed_maint_mult: f64,
    pub defense_maint_mult: f64,
    pub attack_maint_mult: f64,
    pub battery_maint_mult: f64,
}

// Size scaling (supertanker bulk economics)
const SIZE_DEV_BASE: f64 = 50_000.0;
const SIZE_DEV_EXP: f64 = 2.0;
const SIZE_MAINT_BASE: f64 = 100.0;
const SIZE_MAINT_EXP: f64 = 1.0; // Linear maintenance for economies of scale

// Speed scaling (exponential for both)
const SPEED_DEV_BASE: f64 = 8_000_000.0;
const SPEED_DEV_EXP: f64 = 2.5;
const SPEED_MAINT_BASE: f64 = 40_000.0;
const SPEED_MAINT_EXP: f64 = 2.5;

// Attack scaling (linear base, exponential maintenance after 100)
const ATTACK_DEV_BASE: f64 = 100_000.0;
const ATTACK_MAINT_BASE: f64 = 1_000.0;
const ATTACK_SOFT_CAP: f64 = 100.0;
const ATTACK_MAINT_EXP: f64 = 2.0;

// Defense scaling (linear)
const DEFENSE_DEV_BASE: f64 = 25_000.0;
const DEFENSE_MAINT_BASE: f64 = 200.0;

// Battery scaling (linear)
const BATTERY_DEV_BASE: f64 = 100_000.0;
const BATTERY_MAINT_BASE: f64 = 100.0;

// Radar scaling (linear dev, exponential maintenance)
const RADAR_DEV_BASE: f64 = 1_000_000.0;
const RADAR_MAINT_BASE: f64 = 10_000.0;
const RADAR_MAINT_EXP: f64 = 2.0;

// Interconnected scaling factors (Maintenance only)
// Direct multiplicative scaling: final_cost = base_cost × (multiplier_stat / BASELINE_STAT)
const SPEED_SIZE_MAINT_FACTOR: f64 = 1.0; // speed_maint × (size_f / 10) - 10kt baseline
const DEFENSE_SIZE_MAINT_FACTOR: f64 = 1.0; // defense_maint × (size_f / 10)
const ATTACK_SPEED_MAINT_FACTOR: f64 = 1.0; // attack_maint × (speed_f / 0.1) - 0.1 ly/s baseline
const BATTERY_COMBO_MAINT_FACTOR: f64 = 1.0; // battery_maint × (size_f/10 × attack_f/1 × speed_f/0.1)

// Minimum costs & Time scaling
const MIN_DEV_COST: f64 = 1_000_000.0;
const MIN_DEV_TIME_MINUTES: u64 = 1;
const TIME_SCALE_FACTOR: f64 = 0.0001; // minutes per credit

pub fn compute_cost(stats: &ShipStats) -> Result<CostBreakdown, &'static str> {
    stats.validate()?;

    let pow_safe = |val: f64, exp: f64| if val <= 0.0 { 0.0 } else { val.powf(exp) };

    let size_f = stats.size_kt as f64;
    let speed_f = stats.speed_lys;
    let attack_f = stats.attack as f64;
    let defense_f = stats.defense as f64;
    let battery_f = stats.battery_ly as f64;
    let radar_f = stats.radar_ly as f64;

    // Development costs
    let size_dev = if size_f > 0.0 {
        SIZE_DEV_BASE * pow_safe(size_f, SIZE_DEV_EXP)
    } else {
        0.0
    };
    let speed_dev = SPEED_DEV_BASE * pow_safe(speed_f, SPEED_DEV_EXP);
    let attack_dev = ATTACK_DEV_BASE * attack_f;
    let defense_dev = DEFENSE_DEV_BASE * defense_f;
    let battery_dev = BATTERY_DEV_BASE * battery_f;
    let radar_dev = RADAR_DEV_BASE * radar_f;

    let mut total_dev_cost =
        size_dev + speed_dev + attack_dev + defense_dev + battery_dev + radar_dev;
    let dev_padding = if total_dev_cost < MIN_DEV_COST {
        MIN_DEV_COST - total_dev_cost
    } else {
        0.0
    };
    total_dev_cost = total_dev_cost.max(MIN_DEV_COST);

    let time_mult_factor_for_individual_costs = if total_dev_cost > 0.0 && dev_padding > 0.0 {
        total_dev_cost / (total_dev_cost - dev_padding)
    } else {
        1.0
    };

    // Maintenance costs (Base - before multiplication)
    let size_maint_base = if size_f > 0.0 {
        SIZE_MAINT_BASE * pow_safe(size_f, SIZE_MAINT_EXP)
    } else {
        0.0
    };
    let speed_maint_base = SPEED_MAINT_BASE * pow_safe(speed_f, SPEED_MAINT_EXP);

    let attack_maint_base = if attack_f <= ATTACK_SOFT_CAP {
        ATTACK_MAINT_BASE * attack_f
    } else {
        ATTACK_MAINT_BASE * ATTACK_SOFT_CAP
            + ATTACK_MAINT_BASE * pow_safe(attack_f - ATTACK_SOFT_CAP, ATTACK_MAINT_EXP)
    };

    let defense_maint_base = DEFENSE_MAINT_BASE * defense_f;
    let battery_maint_base = BATTERY_MAINT_BASE * battery_f;
    let radar_maint_base = RADAR_MAINT_BASE * pow_safe(radar_f, RADAR_MAINT_EXP);

    // Apply DIRECT MULTIPLICATIVE scaling to maintenance costs
    // Baseline: 10kt size, 0.1 ly/s speed, 1 attack
    const BASELINE_SIZE: f64 = 10.0;
    const BASELINE_SPEED: f64 = 0.1;
    const BASELINE_ATTACK: f64 = 1.0;

    // speed_maint multiplies with normalized size (size_f / 10)
    let speed_maint_mult = (size_f / BASELINE_SIZE) * SPEED_SIZE_MAINT_FACTOR;
    let speed_maint_final = speed_maint_base * speed_maint_mult;

    // defense_maint multiplies with normalized size (size_f / 10)
    let defense_maint_mult = (size_f / BASELINE_SIZE) * DEFENSE_SIZE_MAINT_FACTOR;
    let defense_maint_final = defense_maint_base * defense_maint_mult;

    // attack_maint multiplies with normalized speed (speed_f / 0.1)
    let attack_maint_mult = (speed_f / BASELINE_SPEED) * ATTACK_SPEED_MAINT_FACTOR;
    let attack_maint_final = attack_maint_base * attack_maint_mult;

    // battery_maint multiplies with normalized size × attack × speed
    let battery_maint_mult = (size_f / BASELINE_SIZE)
        * (attack_f.max(1.0) / BASELINE_ATTACK)
        * (speed_f / BASELINE_SPEED)
        * BATTERY_COMBO_MAINT_FACTOR;
    let battery_maint_final = battery_maint_base * battery_maint_mult;

    // Total maintenance cost (size and radar are not multiplied by other stats)
    let total_maint_cost = size_maint_base
        + speed_maint_final
        + attack_maint_final
        + defense_maint_final
        + battery_maint_final
        + radar_maint_base;

    // Time calculations
    let size_time = (size_dev * time_mult_factor_for_individual_costs).round() as u64;
    let speed_time = (speed_dev * time_mult_factor_for_individual_costs).round() as u64;
    let attack_time = (attack_dev * time_mult_factor_for_individual_costs).round() as u64;
    let defense_time = (defense_dev * time_mult_factor_for_individual_costs).round() as u64;
    let battery_time = (battery_dev * time_mult_factor_for_individual_costs).round() as u64;
    let radar_time = (radar_dev * time_mult_factor_for_individual_costs).round() as u64;

    let total_time_raw = (total_dev_cost * TIME_SCALE_FACTOR).round() as u64;
    let total_time = total_time_raw.max(MIN_DEV_TIME_MINUTES);

    Ok(CostBreakdown {
        size_dev_credits: size_dev.round() as u64,
        speed_dev_credits: speed_dev.round() as u64,
        attack_dev_credits: attack_dev.round() as u64,
        defense_dev_credits: defense_dev.round() as u64,
        battery_dev_credits: battery_dev.round() as u64,
        radar_dev_credits: radar_dev.round() as u64,
        total_dev_credits: total_dev_cost.round() as u64,

        size_dev_minutes: size_time,
        speed_dev_minutes: speed_time,
        attack_dev_minutes: attack_time,
        defense_dev_minutes: defense_time,
        battery_dev_minutes: battery_time,
        radar_dev_minutes: radar_time,
        total_dev_minutes: total_time,

        size_maint_base_credits: size_maint_base.round() as u64,
        speed_maint_base_credits: speed_maint_base.round() as u64,
        attack_maint_base_credits: attack_maint_base.round() as u64,
        defense_maint_base_credits: defense_maint_base.round() as u64,
        battery_maint_base_credits: battery_maint_base.round() as u64,
        radar_maint_base_credits: radar_maint_base.round() as u64,

        size_maint_credits: size_maint_base.round() as u64, // Size is base only
        speed_maint_credits: speed_maint_final.round() as u64,
        attack_maint_credits: attack_maint_final.round() as u64,
        defense_maint_credits: defense_maint_final.round() as u64,
        battery_maint_credits: battery_maint_final.round() as u64,
        radar_maint_credits: radar_maint_base.round() as u64, // Radar is base only
        total_maint_credits: total_maint_cost.round() as u64,

        speed_maint_mult,
        defense_maint_mult,
        attack_maint_mult,
        battery_maint_mult,
    })
}
