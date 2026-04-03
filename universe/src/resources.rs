use crate::{generator::PlanetType, hasher::point_to_random};

// Resource seeds
const RESOURCE_SEED: u64 = 0xAAAA_AAAA_AAAA_AAAA;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Iron(f64), // multiplier for iron
    Helium(f64), // multiplier for helium
               // Future materials go here...
}

impl Material {
    pub fn name(&self) -> &'static str {
        match self {
            Material::Iron(_) => "Iron",
            Material::Helium(_) => "Helium",
        }
    }

    pub fn multiplier(&self) -> f64 {
        match self {
            Material::Iron(m) => *m,
            Material::Helium(m) => *m,
        }
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
