use universe::hasher::point_hash;

pub const PLANET_SECRET: u64 = 0xABCD_EF01_2345_6789;

pub fn generate_planet_key(x: i32, y: i32) -> u64 {
    point_hash(x, y, PLANET_SECRET)
}
