use crate::hasher::point_to_random;

const MIN_SPACING: f64 = 500.0;
const GROWTH_RATE: f64 = 2_e-4;
const STAR_EXISTENCE_SEED: u64 = 0xDEADBEEFCAFEBABE;

fn star_probability(x: i32, y: i32) -> f64 {
    let r = ((x * x + y * y) as f64).sqrt();
    let spacing = MIN_SPACING * (GROWTH_RATE * r).exp();
    1.0 / spacing
}

pub fn star_is_at_point(x: i32, y: i32) -> bool {
    point_to_random(x, y, STAR_EXISTENCE_SEED) < star_probability(x, y)
}
