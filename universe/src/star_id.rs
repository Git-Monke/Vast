//! Reversible star labels for grid cells (tenths of a ly per step). Same as server/client coords.
//!
//! Standard form `{Xblock}{Yblock}-{rx}-{ry}` — blocks are **10 000** grid units (Euclidean /
//! `rem_euclid`, remainders **0..10 000**).
//!
//! Block letters (per axis):
//! - **`A`–`Z`**: block **0–25**
//! - **`a`–`z`**: **-1–-26** (`a` = -1 … `z` = -26)
//!
//! If a block is outside **-26..=25**, encoding uses fallback `!x,y` (comma-separated, signed).
//!
//! Use [`parse_star_id`] to recover `(star_x, star_y)` for warp commands.

const BLOCK: i32 = 10_000;

/// Stable id for a star grid cell (matches server `star_location_id`).
#[must_use]
pub fn star_location_id(star_x: i32, star_y: i32) -> u128 {
    u128::from(star_x as u32) | (u128::from(star_y as u32) << 32)
}

/// Map one block index to a letter, or `None` if outside **-26..=25**.
fn block_char(b: i32) -> Option<char> {
    if (0..=25).contains(&b) {
        char::from_u32(b as u32 + u32::from(b'A'))
    } else if (-26..=-1).contains(&b) {
        let idx = (-b - 1) as u32;
        char::from_u32(idx + u32::from(b'a'))
    } else {
        None
    }
}

fn block_index_from_letter(c: char) -> Option<i32> {
    match c {
        'A'..='Z' => Some((c as u8 - b'A') as i32),
        'a'..='z' => Some(-((c as u8 - b'a') as i32 + 1)),
        _ => None,
    }
}

#[must_use]
pub fn star_display_id(star_x: i32, star_y: i32) -> String {
    let bx = star_x.div_euclid(BLOCK);
    let by = star_y.div_euclid(BLOCK);
    let rx = star_x.rem_euclid(BLOCK) as u32;
    let ry = star_y.rem_euclid(BLOCK) as u32;

    match (block_char(bx), block_char(by)) {
        (Some(cx), Some(cy)) => format!("{cx}{cy}-{rx}-{ry}"),
        _ => format!("!{star_x},{star_y}"),
    }
}

/// Decode a string from [`star_display_id`]. Whitespace around the string is trimmed.
#[must_use]
pub fn parse_star_id(s: &str) -> Option<(i32, i32)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('!') {
        let (a, b) = rest.split_once(',')?;
        let x: i32 = a.trim().parse().ok()?;
        let y: i32 = b.trim().parse().ok()?;
        return Some((x, y));
    }

    let mut ch = s.chars();
    let c0 = ch.next()?;
    let c1 = ch.next()?;
    if ch.next()? != '-' {
        return None;
    }
    let rest: String = ch.collect();
    let (rx_s, ry_s) = rest.split_once('-')?;
    let rx: u32 = rx_s.parse().ok()?;
    let ry: u32 = ry_s.parse().ok()?;
    if rx >= BLOCK as u32 || ry >= BLOCK as u32 {
        return None;
    }
    let bx = block_index_from_letter(c0)?;
    let by = block_index_from_letter(c1)?;
    Some((bx * BLOCK + rx as i32, by * BLOCK + ry as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_standard() {
        for (x, y) in [(1000, 1000), (11_000, 1000), (0, 0), (-1000, 1000), (-11_000, -1000)] {
            let id = star_display_id(x, y);
            assert_eq!(parse_star_id(&id), Some((x, y)), "id={id}");
        }
    }

    #[test]
    fn round_trip_fallback() {
        let x = 500_000;
        let y = -300_000;
        let id = star_display_id(x, y);
        assert!(id.starts_with('!'));
        assert_eq!(parse_star_id(&id), Some((x, y)));
    }

    #[test]
    fn examples_from_spec() {
        assert_eq!(star_display_id(1000, 1000), "AA-1000-1000");
        assert_eq!(star_display_id(11_000, 1000), "BA-1000-1000");
    }

    #[test]
    fn star_location_id_packed() {
        assert_eq!(super::star_location_id(0, 0), 0u128);
        assert_eq!(super::star_location_id(-1, 2), u128::from(-1i32 as u32) | (u128::from(2u32) << 32));
    }
}
