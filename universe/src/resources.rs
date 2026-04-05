use crate::{generator::PlanetType, hasher::point_to_random};

// Resource seeds
const RESOURCE_SEED: u64 = 0xAAAA_AAAA_AAAA_AAAA;

/// Tag-only material species (e.g. [`Material::kind`] without caring about the `f64` payload).
#[cfg_attr(feature = "spacetimedb", derive(spacetimedb::SpacetimeType))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialKind {
    Iron,
    Helium,
}

impl MaterialKind {
    /// Every variant, in stable order — update when adding a [`Material`] variant.
    pub const ALL: &'static [MaterialKind] = &[MaterialKind::Iron, MaterialKind::Helium];
}

/// Resource amount or richness, depending on context.
///
/// - **Procedural planets:** `f64` is a **spawn multiplier** (richness) from [`collect_materials`].
/// - **SpacetimeDB warehouses:** `f64` is **stored quantity** (units). Same representation, different meaning.
#[cfg_attr(feature = "spacetimedb", derive(spacetimedb::SpacetimeType))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Iron(f64),
    Helium(f64),
    // Future materials go here...
}

impl Material {
    pub fn kind(&self) -> MaterialKind {
        match self {
            Material::Iron(_) => MaterialKind::Iron,
            Material::Helium(_) => MaterialKind::Helium,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Material::Iron(_) => "Iron",
            Material::Helium(_) => "Helium",
        }
    }

    /// Inner `f64`: vein multiplier, stored kt, etc. Update the match when adding a variant.
    #[inline]
    #[must_use]
    pub fn amount(&self) -> f64 {
        match self {
            Material::Iron(m) | Material::Helium(m) => *m,
        }
    }

    /// Same as [`amount`](Self::amount); kept for procedural “richness” call sites.
    #[inline]
    #[must_use]
    pub fn multiplier(&self) -> f64 {
        self.amount()
    }
}

fn spawn_iron(temp_k: f64, p_type: PlanetType, x: i32, y: i32, idx: u8) -> Option<Material> {
    if matches!(p_type, PlanetType::Solid) && temp_k < 1000.0 {
        let mult = 1.0 + point_to_random(x, y, RESOURCE_SEED.wrapping_add(idx as u64)) * 2.0;
        Some(Material::Iron(mult))
    } else {
        None
    }
}

fn spawn_helium(p_type: PlanetType, x: i32, y: i32, idx: u8) -> Option<Material> {
    if matches!(p_type, PlanetType::Gas) {
        let mult = 1.0 + point_to_random(x, y, RESOURCE_SEED.wrapping_add(idx as u64)) * 4.0;
        Some(Material::Helium(mult))
    } else {
        None
    }
}

/// Collects all materials that satisfy their spawn conditions for a planet
pub fn collect_materials(
    temp_k: f64,
    p_type: PlanetType,
    x: i32,
    y: i32,
    idx: u8,
) -> Vec<Material> {
    let mut materials = Vec::new();

    if let Some(m) = spawn_iron(temp_k, p_type, x, y, idx) {
        materials.push(m);
    }

    if let Some(m) = spawn_helium(p_type, x, y, idx) {
        materials.push(m);
    }

    // Add more material spawn calls here when new materials are introduced
    materials
}
