/// Credits granted when an empire first registers.
pub const STARTING_CREDITS: u64 = 10_000;
pub const MAX_EMPIRE_NAME_LEN: usize = 64;

/// New players spawn at a random point within this Euclidean radius (light-years) of the galactic origin.
pub const STARTER_DISK_RADIUS_LY: f64 = 5_000.0;

/// Random disk samples tried before giving up (empty Red dwarf + planet with no buildings is sparse).
pub const MAX_STARTER_SAMPLE_ATTEMPTS: u32 = 4_096;

/// After each disk sample, search this many cells per side (centered on the sample anchor).
pub const STARTER_LOCAL_GRID: i32 = 50;
pub const STARTER_LOCAL_HALF: i32 = STARTER_LOCAL_GRID / 2;
