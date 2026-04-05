//! Merged warehouse / cargo amounts as [`Vec<Material>`] (kt per species).
//! When adding a new [`Material`] variant, update [`MaterialKind::ALL`](crate::MaterialKind::ALL)
//! and [`material_from_kind_kt`](material_from_kind_kt).

use std::collections::HashMap;

use crate::{Material, MaterialKind};

/// Sum of kt across all stacked materials.
#[must_use]
pub fn total_kt(stock: &[Material]) -> f64 {
    stock.iter().map(Material::amount).sum()
}

#[must_use]
pub fn get_amount(stock: &[Material], kind: MaterialKind) -> f64 {
    stock
        .iter()
        .filter(|m| m.kind() == kind)
        .map(Material::amount)
        .sum()
}

/// One [`Material`] row for `kind` with `kt` (for positive amounts).
#[must_use]
pub fn material_from_kind_kt(kind: MaterialKind, kt: f64) -> Material {
    match kind {
        MaterialKind::Iron => Material::Iron(kt),
        MaterialKind::Helium => Material::Helium(kt),
    }
}

/// Merge `kt` into `stock` for `kind` (adds a row or increases existing).
pub fn merge_add_kt(stock: &mut Vec<Material>, kind: MaterialKind, kt: f64) {
    if kt.abs() < 1e-18 {
        return;
    }
    for m in stock.iter_mut() {
        if m.kind() == kind {
            *m = material_from_kind_kt(kind, m.amount() + kt);
            return;
        }
    }
    stock.push(material_from_kind_kt(kind, kt));
}

/// Collapse duplicate kinds into one entry each; drop near-zero rows.
pub fn normalize_material_vec(stock: &mut Vec<Material>) {
    let mut map: HashMap<MaterialKind, f64> = HashMap::new();
    for m in stock.drain(..) {
        let k = m.kind();
        let q = m.amount();
        *map.entry(k).or_insert(0.0) += q;
    }
    let mut kinds: Vec<MaterialKind> = map.keys().copied().collect();
    kinds.sort_by_key(|k| kind_sort_index(*k));
    for k in kinds {
        let q = map[&k];
        if q > 1e-12 {
            stock.push(material_from_kind_kt(k, q));
        }
    }
}

fn kind_sort_index(k: MaterialKind) -> u8 {
    MaterialKind::ALL
        .iter()
        .position(|x| *x == k)
        .unwrap_or(255) as u8
}

/// Scale every amount so `total_kt <= capacity_kt` (proportional if over).
pub fn clamp_settled_to_capacity(stock: &mut Vec<Material>, capacity_kt: f64) {
    normalize_material_vec(stock);
    let sum = total_kt(stock);
    if sum <= capacity_kt || sum <= 0.0 {
        return;
    }
    let s = capacity_kt / sum;
    for m in stock.iter_mut() {
        let k = m.kind();
        *m = material_from_kind_kt(k, m.amount() * s);
    }
    normalize_material_vec(stock);
}

/// Aggregate mining depot output: `kind -> kt/s`.
#[must_use]
pub fn mining_rates_hash_from_pairs(
    iter: impl Iterator<Item = (MaterialKind, f64)>,
) -> HashMap<MaterialKind, f64> {
    let mut m: HashMap<MaterialKind, f64> = HashMap::new();
    for (k, r) in iter {
        *m.entry(k).or_insert(0.0) += r;
    }
    m
}

#[must_use]
pub fn total_rate_kt_s(rates: &HashMap<MaterialKind, f64>) -> f64 {
    rates.values().sum()
}

/// Apply accrual `delta_kt = rate * t_eff` per kind, then normalize and clamp to capacity.
pub fn accrue_settled(
    settled: &mut Vec<Material>,
    rates: &HashMap<MaterialKind, f64>,
    t_eff: f64,
    capacity_kt: f64,
) {
    for (&k, &r) in rates {
        merge_add_kt(settled, k, r * t_eff);
    }
    normalize_material_vec(settled);
    clamp_settled_to_capacity(settled, capacity_kt);
}

/// Theoretical totals after accrual (same math as settlement) without mutating stored row.
#[must_use]
pub fn theoretical_materials_after_accrual(
    settled: &[Material],
    rates: &HashMap<MaterialKind, f64>,
    t_eff: f64,
    capacity_kt: f64,
) -> Vec<Material> {
    let mut v = settled.to_vec();
    accrue_settled(&mut v, rates, t_eff, capacity_kt);
    v
}

/// Subtract `pickup` from `stock` if every kind has enough; normalize after.
pub fn try_subtract_materials(stock: &mut Vec<Material>, pickup: &[Material]) -> Result<(), String> {
    normalize_material_vec(stock);
    let mut need: HashMap<MaterialKind, f64> = HashMap::new();
    for p in pickup {
        let k = p.kind();
        let q = p.amount();
        if q < 0.0 {
            return Err("Pickup amounts must be non-negative".to_string());
        }
        if q > 0.0 {
            *need.entry(k).or_insert(0.0) += q;
        }
    }
    for (&k, q) in &need {
        if get_amount(stock, k) + 1e-9 < *q {
            return Err("Not enough resources in system warehouse".to_string());
        }
    }
    for (&k, q) in &need {
        merge_add_kt(stock, k, -*q);
    }
    normalize_material_vec(stock);
    Ok(())
}

/// Merge `additions` into `cargo` (ship hold).
pub fn merge_into_cargo(cargo: &mut Vec<Material>, additions: &[Material]) {
    for p in additions {
        let k = p.kind();
        let q = p.amount();
        if q > 0.0 {
            merge_add_kt(cargo, k, q);
        }
    }
    normalize_material_vec(cargo);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_and_merge() {
        let mut v = vec![Material::Iron(1.0), Material::Helium(2.0)];
        assert!((total_kt(&v) - 3.0).abs() < 1e-9);
        merge_add_kt(&mut v, MaterialKind::Iron, 0.5);
        normalize_material_vec(&mut v);
        assert!((get_amount(&v, MaterialKind::Iron) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn clamp_scales() {
        let mut v = vec![Material::Iron(10.0), Material::Helium(10.0)];
        clamp_settled_to_capacity(&mut v, 15.0);
        assert!((total_kt(&v) - 15.0).abs() < 1e-6);
    }

    #[test]
    fn subtract_ok() {
        let mut v = vec![Material::Iron(5.0)];
        try_subtract_materials(&mut v, &[Material::Iron(2.0)]).unwrap();
        assert!((get_amount(&v, MaterialKind::Iron) - 3.0).abs() < 1e-9);
    }
}
