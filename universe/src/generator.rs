use crate::{
    checker::star_is_at_point,
    hasher::{point_hash, point_to_random},
    resources::{Material, collect_materials},
};

const STAR_TYPE_SEED: u64 = 0x1111_1111_1111_1111;
const STAR_SIZE_SEED: u64 = 0x6666_6666_6666_6666;
const PLANET_COUNT_SEED: u64 = 0x2222_2222_2222_2222;
const PLANET_DIST_SEED: u64 = 0x7777_7777_7777_7777;
const PLANET_TYPE_SEED: u64 = 0x5555_5555_5555_5555;
const PLANET_SIZE_SEED: u64 = 0x3333_3333_3333_3333;
const PLANET_RICHNESS_SEED: u64 = 0x4444_4444_4444_4444;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarType {
    Red,
    Orange,
    Yellow,
    YellowWhite,
    White,
    BlueWhite,
    Blue,
    NeutronStar,
}

impl StarType {
    fn from_index(i: usize) -> Self {
        match i {
            0 => StarType::Red,
            1 => StarType::Orange,
            2 => StarType::Yellow,
            3 => StarType::YellowWhite,
            4 => StarType::White,
            5 => StarType::BlueWhite,
            6 => StarType::Blue,
            _ => StarType::NeutronStar,
        }
    }

    pub fn temperature_k(&self) -> f64 {
        match self {
            StarType::Red => 3_200.0,
            StarType::Orange => 4_500.0,
            StarType::Yellow => 5_800.0,
            StarType::YellowWhite => 7_000.0,
            StarType::White => 9_500.0,
            StarType::BlueWhite => 15_000.0,
            StarType::Blue => 30_000.0,
            StarType::NeutronStar => 600_000.0,
        }
    }

    // Returns (min, max) in solar radii
    fn size_range_solar_radii(&self) -> (f64, f64) {
        match self {
            StarType::Red => (0.10, 0.50),
            StarType::Orange => (0.50, 0.90),
            StarType::Yellow => (0.90, 1.20),
            StarType::YellowWhite => (1.20, 1.80),
            StarType::White => (1.80, 2.50),
            StarType::BlueWhite => (2.50, 8.00),
            StarType::Blue => (8.00, 20.00),
            StarType::NeutronStar => (0.000014, 0.000021), // ~10–15 km radius
        }
    }

    // Returns (min, max) richness multiplier for planets in this system.
    // Red dwarfs are poor; neutron stars bathe planets in exotic radiation
    // making minerals incredibly dense — multipliers are far off the normal scale.
    fn richness_range(&self) -> (f64, f64) {
        match self {
            StarType::Red => (0.2, 0.6),
            StarType::Orange => (0.6, 1.2),
            StarType::Yellow => (1.2, 2.0),
            StarType::YellowWhite => (2.0, 4.0),
            StarType::White => (4.0, 8.0),
            StarType::BlueWhite => (8.0, 16.0),
            StarType::Blue => (30.0, 50.0),
            StarType::NeutronStar => (200.0, 500.0),
        }
    }
}

#[cfg_attr(feature = "spacetimedb", derive(spacetimedb::SpacetimeType))]
#[derive(Clone, Copy, Debug)]
pub enum PlanetType {
    Solid,
    Ocean,
    Gas,
}

pub struct StarSystem {
    pub x: i32,
    pub y: i32,
    pub name: String,
    pub star_type: StarType,
    pub star_size_solar_radii: f64,
    pub planets: Vec<Planet>,
}

#[derive(Clone, Debug)]
pub struct Planet {
    pub index: u8,
    pub name: String,
    pub distance_au: f64,
    pub temperature_k: f64,
    pub planet_type: PlanetType,
    pub size: u8,      // buildable slots, 1–10
    pub richness: f64, // multiplier, e.g. 1.5× base yield
    pub resources: Vec<Material>,
}

macro_rules! thresh {
    ($num:literal / $den:literal) => {
        (u64::MAX as u128 * $num / $den) as u64
    };
}

const PLANET_COUNT_THRESHOLDS: [u64; 9] = [
    thresh!(5 / 100),
    thresh!(15 / 100),
    thresh!(40 / 100),
    thresh!(70 / 100),
    thresh!(85 / 100),
    thresh!(93 / 100),
    thresh!(965 / 1000),
    thresh!(985 / 1000),
    thresh!(9999 / 10000),
];

const STAR_TYPE_THRESHOLDS: [u64; 7] = [
    thresh!(73 / 100),   // Red
    thresh!(86 / 100),   // Orange
    thresh!(93 / 100),   // Yellow
    thresh!(96 / 100),   // YellowWhite
    thresh!(98 / 100),   // White
    thresh!(995 / 1000), // BlueWhite
    thresh!(99999 / 100000), // Blue
                         // Neutron otherwise — extremely rare
];

fn hash_to_planet_count(hash: u64) -> u8 {
    for (i, &t) in PLANET_COUNT_THRESHOLDS.iter().enumerate() {
        if hash < t {
            return (i + 1) as u8;
        }
    }
    10
}

fn hash_to_star_type(hash: u64) -> StarType {
    for (i, &t) in STAR_TYPE_THRESHOLDS.iter().enumerate() {
        if hash < t {
            return StarType::from_index(i);
        }
    }
    StarType::NeutronStar
}

fn planet_temperature_k(star_temp_k: f64, orbit_index: u8) -> f64 {
    star_temp_k * 0.12 / (orbit_index as f64 + 1.0).powf(0.75)
}

fn planet_type_from_temp(temp_k: f64, type_random: f64) -> PlanetType {
    if temp_k > 700.0 {
        PlanetType::Solid
    } else if temp_k > 250.0 {
        if temp_k <= 350.0 && type_random < 0.45 {
            PlanetType::Ocean
        } else {
            PlanetType::Solid
        }
    } else if temp_k > 100.0 {
        if type_random < 0.55 {
            PlanetType::Gas
        } else {
            PlanetType::Solid
        }
    } else {
        PlanetType::Gas
    }
}

/// Cheap render-time query: returns (star_type, size_solar_radii) without building planets.
pub fn star_info_at(x: i32, y: i32) -> Option<(StarType, f64)> {
    if !star_is_at_point(x, y) {
        return None;
    }
    let star_type = hash_to_star_type(point_hash(x, y, STAR_TYPE_SEED));
    let (sz_min, sz_max) = star_type.size_range_solar_radii();
    let size = sz_min + (sz_max - sz_min) * point_to_random(x, y, STAR_SIZE_SEED);
    Some((star_type, size))
}

pub fn generate_star(x: i32, y: i32, key: Option<u64>) -> Option<StarSystem> {
    if !star_is_at_point(x, y) {
        return None;
    }

    let star_type = hash_to_star_type(point_hash(x, y, STAR_TYPE_SEED));
    
    let (sz_min, sz_max) = star_type.size_range_solar_radii();
    let star_size_solar_radii = sz_min + (sz_max - sz_min) * point_to_random(x, y, STAR_SIZE_SEED);

    let mut planets = Vec::new();

    if let Some(k) = key {
        let planet_count = hash_to_planet_count(point_hash(x, y, PLANET_COUNT_SEED ^ k));
        let star_temp = star_type.temperature_k();
        let (r_min, r_max) = star_type.richness_range();

        // First orbit: 0.1–0.5 AU; each successive orbit multiplies by 1.4–2.0
        let mut distance_au = 0.1 + point_to_random(x, y, PLANET_DIST_SEED ^ k) * 0.4;
        planets.reserve(planet_count as usize);

        for i in 0..planet_count {
            if i > 0 {
                let spacing =
                    1.4 + point_to_random(x, y, (PLANET_DIST_SEED ^ k).wrapping_add(i as u64)) * 0.6;
                distance_au *= spacing;
            }

            let temp_k = planet_temperature_k(star_temp, i);
            let type_random = point_to_random(x, y, (PLANET_TYPE_SEED ^ k).wrapping_add(i as u64));
            let planet_type = planet_type_from_temp(temp_k, type_random);

            let size_random = point_to_random(x, y, (PLANET_SIZE_SEED ^ k).wrapping_add(i as u64));
            let size: u8 = (match planet_type {
                PlanetType::Gas => 1.0 + size_random * 4.0, // 1–5 orbital platforms
                PlanetType::Ocean => 4.0 + size_random * 4.0, // 4–8 large habitable surface
                PlanetType::Solid => 2.0 + size_random * 8.0, // 2–10 most variable
            } as u8)
                .max(1);

            let richness = r_min
                + (r_max - r_min) * point_to_random(x, y, (PLANET_RICHNESS_SEED ^ k).wrapping_add(i as u64));

            let resources = collect_materials(temp_k, planet_type, x, y, i, k);

            planets.push(Planet {
                index: i,
                name: String::new(),
                distance_au,
                temperature_k: temp_k,
                planet_type,
                size,
                richness,
                resources,
            });
        }
    }

    Some(StarSystem {
        x,
        y,
        name: String::new(),
        star_type,
        star_size_solar_radii,
        planets,
    })
}
