/// Maximum building level supported by [`MIN_SHIP_KT_FOR_LEVEL`] and credit tables.
pub const MAX_BUILDING_LEVEL: usize = 12;

/// Minimum ship `size_kt` by **level** (level 1 → `MIN_SHIP_KT_FOR_LEVEL[0]`, minimum 1 kt).
pub const MIN_SHIP_KT_FOR_LEVEL: [u32; MAX_BUILDING_LEVEL] =
    [1, 5, 10, 20, 50, 100, 150, 200, 250, 300, 400, 500];

#[derive(Copy, Clone)]
pub struct GarrisonStats {
    pub attack: u32,
    pub defense: u32,
    pub speed_lys: f64,
    pub health: u32,
}

pub const GARRISON_STATS_FOR_LEVEL: [GarrisonStats; MAX_BUILDING_LEVEL] = [
    GarrisonStats {
        attack: 5,
        defense: 10,
        speed_lys: 0.1,
        health: 100,
    }, // 1
    GarrisonStats {
        attack: 8,
        defense: 16,
        speed_lys: 0.2,
        health: 180,
    }, // 2
    GarrisonStats {
        attack: 13,
        defense: 26,
        speed_lys: 0.4,
        health: 300,
    }, // 3
    GarrisonStats {
        attack: 20,
        defense: 40,
        speed_lys: 0.7,
        health: 500,
    }, // 4
    GarrisonStats {
        attack: 35,
        defense: 70,
        speed_lys: 1.2,
        health: 850,
    }, // 5
    GarrisonStats {
        attack: 60,
        defense: 120,
        speed_lys: 2.0,
        health: 1400,
    }, // 6
    GarrisonStats {
        attack: 100,
        defense: 200,
        speed_lys: 3.2,
        health: 2400,
    }, // 7
    GarrisonStats {
        attack: 170,
        defense: 340,
        speed_lys: 5.0,
        health: 4000,
    }, // 8
    GarrisonStats {
        attack: 280,
        defense: 560,
        speed_lys: 7.0,
        health: 6500,
    }, // 9
    GarrisonStats {
        attack: 450,
        defense: 900,
        speed_lys: 10.0,
        health: 10000,
    }, // 10
    GarrisonStats {
        attack: 700,
        defense: 1400,
        speed_lys: 12.0,
        health: 14000,
    }, // 11
    GarrisonStats {
        attack: 1000,
        defense: 2000,
        speed_lys: 15.0,
        health: 20000,
    }, // 12
];
