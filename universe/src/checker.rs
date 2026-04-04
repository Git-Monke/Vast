//! Star placement: Bernoulli trial per 0.1 ly cell with radius-dependent mean spacing.

use crate::hasher::point_to_random;
use crate::settings::{
    distance_from_origin_ly, mean_spacing_at_radius_ly, CELL_SIZE_LY, PLANE_DENSITY_SCALE,
    UNIVERSE_RADIUS_LY,
};

const STAR_EXISTENCE_SEED: u64 = 0xDEADBEEFCAFEBABE;

/// Probability that a star exists at integer grid `(x, y)` (tenths of a ly).
fn star_probability(x: i32, y: i32) -> f64 {
    let r_ly = distance_from_origin_ly(x, y);
    if r_ly > UNIVERSE_RADIUS_LY {
        return 0.0;
    }
    let spacing = mean_spacing_at_radius_ly(r_ly);
    ((CELL_SIZE_LY / spacing) * PLANE_DENSITY_SCALE).min(1.0)
}

pub fn star_is_at_point(x: i32, y: i32) -> bool {
    point_to_random(x, y, STAR_EXISTENCE_SEED) < star_probability(x, y)
}

/// Expected number of stars in the universe disk (integral of per-cell probabilities).
/// Used for tests and balance; not used at runtime for generation.
pub fn expected_star_count_integral() -> f64 {
    const STEPS: usize = 50_000;
    let r_max = UNIVERSE_RADIUS_LY;
    let mut sum = 0.0_f64;
    for i in 0..STEPS {
        let r0 = r_max * i as f64 / STEPS as f64;
        let r1 = r_max * (i + 1) as f64 / STEPS as f64;
        let r_mid = (r0 + r1) * 0.5;
        let spacing = mean_spacing_at_radius_ly(r_mid);
        let p = ((CELL_SIZE_LY / spacing) * PLANE_DENSITY_SCALE).min(1.0);
        let area = std::f64::consts::PI * (r1 * r1 - r0 * r0);
        let cells = area / (CELL_SIZE_LY * CELL_SIZE_LY);
        sum += cells * p;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_total_stars_sane_for_two_d_plane() {
        let n = expected_star_count_integral();
        assert!(
            (500_000.0..=20_000_000.0).contains(&n),
            "expected ~{n:.0} stars with PLANE_DENSITY_SCALE; tune spacing or scale in settings"
        );
    }
}
