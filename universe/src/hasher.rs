pub fn point_hash(x: i32, y: i32, seed: u64) -> u64 {
    let mut n = (x as u64).wrapping_mul(2654435761) ^ (y as u64).wrapping_mul(2246822519) ^ seed;
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n = n ^ (n >> 31);
    n
}
pub fn point_to_random(x: i32, y: i32, seed: u64) -> f64 {
    point_hash(x, y, seed) as f64 / u64::MAX as f64
}
