//! Global universe scale: coordinate units, disk radius, and star-density parameters.

/// One integer coordinate step = **0.1 light-year** (grid stores tenths of a ly).
pub const COORD_UNITS_PER_LY: i32 = 10;

/// Light-years from origin to disk edge (universe diameter = 2 × this).
pub const UNIVERSE_RADIUS_LY: f64 = 50_000.0;

/// Target mean spacing between stars at the galactic core (light-years).
pub const CORE_MEAN_SPACING_LY: f64 = 5.0;

/// Target mean spacing at [`UNIVERSE_RADIUS_LY`] (light-years); within the 10–20 ly design band.
pub const EDGE_MEAN_SPACING_LY: f64 = 15.0;

/// Grid cell size in light-years (0.1 ly per step).
pub const CELL_SIZE_LY: f64 = 1.0 / COORD_UNITS_PER_LY as f64;

/// The galaxy is a **2D grid**, not a 3D volume: naive density would pack far too many systems.
/// Applied to every star-placement probability. Order of **~10⁻⁶** matches “~1000× thinner
/// than a 3D-style fill” **twice** (2D slice + playable total count); tune here for balance.
pub const PLANE_DENSITY_SCALE: f64 = 1.0 / 1000.0;

/// `spacing(r) = CORE_MEAN_SPACING_LY * exp(SPACING_GROWTH_PER_LY * r)` for `r` in ly.
/// `ln(EDGE/CORE) / R_MAX` with `EDGE/CORE = 50`.
pub const SPACING_GROWTH_PER_LY: f64 = 3.912023005428146 / UNIVERSE_RADIUS_LY;

#[inline]
pub fn grid_to_ly(g: i32) -> f64 {
    g as f64 / COORD_UNITS_PER_LY as f64
}

#[inline]
pub fn ly_to_grid(ly: f64) -> i32 {
    (ly * COORD_UNITS_PER_LY as f64).round() as i32
}

/// Euclidean distance from origin in light-years (`x`, `y` in tenths of a ly).
#[inline]
pub fn distance_from_origin_ly(x: i32, y: i32) -> f64 {
    let x_ly = grid_to_ly(x);
    let y_ly = grid_to_ly(y);
    (x_ly * x_ly + y_ly * y_ly).sqrt()
}

/// Mean inter-star spacing in ly at radius `r_ly` from the core.
#[inline]
pub fn mean_spacing_at_radius_ly(r_ly: f64) -> f64 {
    CORE_MEAN_SPACING_LY * (SPACING_GROWTH_PER_LY * r_ly).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_endpoints() {
        let s0 = mean_spacing_at_radius_ly(0.0);
        assert!((s0 - CORE_MEAN_SPACING_LY).abs() < 1e-9);
        let s1 = mean_spacing_at_radius_ly(UNIVERSE_RADIUS_LY);
        assert!((s1 - EDGE_MEAN_SPACING_LY).abs() < 1e-6);
    }
}
