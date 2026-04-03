#[derive(Clone, Debug, PartialEq)]
pub struct ShipStats {
    pub size_kt: u32,           // cargo capacity in kilotonnes
    pub speed_tenths_ly_s: u32, // light-years per second * 10 (1 = 0.1 ly/s)
    pub defense: u32,           // hit points required to destroy
    pub attack: u32,            // damage per volley
    pub battery_ly: u32,        // jump distance before recharge
    pub radar_ly: u32,          // scanning range in light-years
}

impl Default for ShipStats {
    fn default() -> Self {
        Self {
            size_kt: 10,
            speed_tenths_ly_s: 1, // 0.1 ly/s
            defense: 10,
            attack: 0,
            battery_ly: 10,
            radar_ly: 5,
        }
    }
}

impl ShipStats {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.speed_tenths_ly_s < 1 {
            return Err("Speed must be at least 0.1 ly/s (value of 1)");
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

    pub size_maint_credits: u64,
    pub speed_maint_credits: u64,
    pub attack_maint_credits: u64,
    pub defense_maint_credits: u64,
    pub battery_maint_credits: u64,
    pub radar_maint_credits: u64,
    pub total_maint_credits: u64,
}

// Size scaling (supertanker bulk economics)
const SIZE_DEV_BASE: f64 = 50_000.0;
const SIZE_DEV_EXP: f64 = 2.0;
const SIZE_MAINT_BASE: f64 = 100.0;
const SIZE_MAINT_EXP: f64 = 0.7;

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
const BATTERY_DEV_BASE: f64 = 50_000.0;
const BATTERY_MAINT_BASE: f64 = 100.0;

// Radar scaling (linear dev, exponential maintenance)
const RADAR_DEV_BASE: f64 = 1_000_000.0;
const RADAR_MAINT_BASE: f64 = 10_000.0;
const RADAR_MAINT_EXP: f64 = 2.0;

// Minimum costs & Time scaling
const MIN_DEV_COST: f64 = 1_000_000.0;
const MIN_DEV_TIME_MINUTES: u64 = 1;
const TIME_SCALE_FACTOR: f64 = 0.0001; // minutes per credit

pub fn compute_cost(stats: &ShipStats) -> Result<CostBreakdown, &'static str> {
    stats.validate()?;

    let pow_safe = |val: f64, exp: f64| if val <= 0.0 { 0.0 } else { val.powf(exp) };

    let size_f = stats.size_kt as f64;
    let speed_f = (stats.speed_tenths_ly_s as f64) / 10.0;
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

    // Distribute the padding proportionally if we want accurate time scaling,
    // or just lump it into the total. We'll just leave individual dev costs as is,
    // but time will be based on the individual dev costs, scaled up slightly if padded.
    let time_mult = TIME_SCALE_FACTOR
        * if total_dev_cost > 0.0 && dev_padding > 0.0 {
            total_dev_cost / (total_dev_cost - dev_padding) // scale up individual times
        } else {
            1.0
        };

    // Maintenance costs
    let size_maint = if size_f > 0.0 {
        SIZE_MAINT_BASE * pow_safe(size_f, SIZE_MAINT_EXP)
    } else {
        0.0
    };
    let speed_maint = SPEED_MAINT_BASE * pow_safe(speed_f, SPEED_MAINT_EXP);

    let attack_maint = if attack_f <= ATTACK_SOFT_CAP {
        ATTACK_MAINT_BASE * attack_f
    } else {
        ATTACK_MAINT_BASE * ATTACK_SOFT_CAP
            + ATTACK_MAINT_BASE * pow_safe(attack_f - ATTACK_SOFT_CAP, ATTACK_MAINT_EXP)
    };

    let defense_maint = DEFENSE_MAINT_BASE * defense_f;
    let battery_maint = BATTERY_MAINT_BASE * battery_f;
    let radar_maint = RADAR_MAINT_BASE * pow_safe(radar_f, RADAR_MAINT_EXP);

    let total_maint_cost =
        size_maint + speed_maint + attack_maint + defense_maint + battery_maint + radar_maint;

    // Time calculations
    let size_time = (size_dev * time_mult).round() as u64;
    let speed_time = (speed_dev * time_mult).round() as u64;
    let attack_time = (attack_dev * time_mult).round() as u64;
    let defense_time = (defense_dev * time_mult).round() as u64;
    let battery_time = (battery_dev * time_mult).round() as u64;
    let radar_time = (radar_dev * time_mult).round() as u64;

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

        size_maint_credits: size_maint.round() as u64,
        speed_maint_credits: speed_maint.round() as u64,
        attack_maint_credits: attack_maint.round() as u64,
        defense_maint_credits: defense_maint.round() as u64,
        battery_maint_credits: battery_maint.round() as u64,
        radar_maint_credits: radar_maint.round() as u64,
        total_maint_credits: total_maint_cost.round() as u64,
    })
}
