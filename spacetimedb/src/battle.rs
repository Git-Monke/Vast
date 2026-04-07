use std::collections::HashMap;

use crate::Building;
use crate::Ship;
use crate::building_rules::garrison_stats;

#[derive(PartialEq, Eq, Hash)]
pub enum CombatantId {
    Ship(u64),
    Garrison(u64),
}

pub struct CombatantResult {
    pub id: CombatantId,
    pub damage_taken: u32,
}

pub trait Combatant {
    fn id(&self) -> CombatantId;
    fn attack(&self) -> u32;
    fn defense(&self) -> u32;
    fn health(&self) -> u32;
    fn speed(&self) -> f64;
}

impl Combatant for Ship {
    fn id(&self) -> CombatantId {
        CombatantId::Ship(self.id)
    }
    fn attack(&self) -> u32 {
        self.stats.attack
    }
    fn defense(&self) -> u32 {
        self.stats.defense
    }
    fn health(&self) -> u32 {
        self.health
    }
    fn speed(&self) -> f64 {
        self.stats.speed_lys
    }
}

impl Combatant for Building {
    fn id(&self) -> CombatantId {
        CombatantId::Garrison(self.id)
    }
    fn attack(&self) -> u32 {
        garrison_stats(self.level as usize).attack
    }
    fn defense(&self) -> u32 {
        garrison_stats(self.level as usize).defense
    }
    fn health(&self) -> u32 {
        self.health
    }
    fn speed(&self) -> f64 {
        garrison_stats(self.level as usize).speed_lys
    }
}

type InProgressResults = HashMap<CombatantId, CombatantResult>;

fn team_attack_power(team: &[&dyn Combatant], results: &InProgressResults) -> u32 {
    team.iter()
        .map(|f| {
            ship_still_alive(*f, results)
                .then(|| f.attack())
                .unwrap_or(0)
        })
        .sum()
}

// id, defense
fn get_team_defender<'a>(
    team: &[&'a dyn Combatant],
    results: &InProgressResults,
) -> &'a dyn Combatant {
    let id = team
        .iter()
        .copied()
        .filter(|f| ship_still_alive(*f, results))
        .max_by(|a, b| a.speed().total_cmp(&b.speed()))
        .map(|c| c.id())
        .unwrap();

    ship_from_id(team, id).unwrap()
}

fn ship_still_alive(ship: &dyn Combatant, results: &InProgressResults) -> bool {
    results
        .get(&ship.id())
        .map_or(true, |r| r.damage_taken < ship.health())
}

fn team_still_alive(team: &[&dyn Combatant], results: &InProgressResults) -> bool {
    team.iter().copied().any(|f| ship_still_alive(f, results))
}

fn actual_damage_dealt(attack_power: u32, defense: u32) -> u32 {
    let ratio = defense as f64 / (attack_power as f64 * 10.0);
    let factor = (1.0 - ratio).max(0.0);
    (attack_power as f64 * factor * factor).floor() as u32
}

fn ship_from_id<'a>(team: &[&'a dyn Combatant], id: CombatantId) -> Option<&'a dyn Combatant> {
    team.iter().copied().find(|f| f.id() == id)
}

fn apply_damage(results: &mut InProgressResults, defender: &dyn Combatant, attack_power: u32) {
    let id = defender.id();

    results
        .entry(id)
        .and_modify(|r| r.damage_taken += attack_power)
        .or_insert(CombatantResult {
            id: defender.id(),
            damage_taken: attack_power,
        });
}

pub fn run_battle(team_1: &[&dyn Combatant], team_2: &[&dyn Combatant]) -> Vec<CombatantResult> {
    let mut results: InProgressResults = HashMap::new();

    while team_still_alive(team_1, &results) && team_still_alive(team_2, &results) {
        let t1_defender = get_team_defender(team_1, &results);
        let t2_defender = get_team_defender(team_2, &results);

        let team_1_attack = team_attack_power(team_1, &results);
        let team_2_attack = team_attack_power(team_2, &results);

        // If both teams have 10x more defense than the opposing attack, it's a stalemate.
        if t1_defender.defense() > team_2_attack * 10 && t2_defender.defense() > team_1_attack * 10
        {
            break;
        }

        let t1_actual_attack =
            actual_damage_dealt(team_1_attack, t2_defender.defense()).min(t2_defender.health());
        let t2_actual_attack =
            actual_damage_dealt(team_2_attack, t1_defender.defense()).min(t1_defender.health());

        apply_damage(&mut results, t1_defender, t2_actual_attack);
        apply_damage(&mut results, t2_defender, t1_actual_attack);
    }

    results.into_values().collect::<Vec<CombatantResult>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildingKind;
    use crate::building_rules::garrison_stats;
    use crate::{Building, Ship};
    use spacetimedb::{Identity, Timestamp};
    use universe::{ShipAttackMode, ShipStats};

    fn test_ship(id: u64, stats: ShipStats, health: u32) -> Ship {
        Ship {
            id,
            owner: Identity::ZERO,
            stats,
            cargo: vec![],
            attack_mode: ShipAttackMode::Defend,
            in_transit: false,
            star_x: 0,
            star_y: 0,
            transit_from_x: 0,
            transit_from_y: 0,
            transit_to_x: 0,
            transit_to_y: 0,
            transit_depart_at: Timestamp::UNIX_EPOCH,
            transit_arrive_at: Timestamp::UNIX_EPOCH,
            jump_ready_at: Timestamp::UNIX_EPOCH,
            health,
        }
    }

    fn test_garrison(id: u64, level: u32, health: u32) -> Building {
        Building {
            id,
            star_x: 0,
            star_y: 0,
            planet_index: 0,
            slot_index: 0,
            kind: BuildingKind::MilitaryGarrison,
            level,
            degradation_percent: 0.0,
            mining_material: None,
            owner: Some(Identity::ZERO),
            attack_mode: Some(ShipAttackMode::Defend),
            health,
        }
    }

    fn damage_for(results: &[CombatantResult], id: CombatantId) -> u32 {
        results
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.damage_taken)
            .unwrap_or(0)
    }

    #[test]
    fn run_battle_produces_non_empty_when_fight_happens() {
        let killer = test_ship(
            1,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 100,
                battery_ly: 50,
                radar_ly: 5,
            },
            100,
        );
        let victim = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 1,
                battery_ly: 50,
                radar_ly: 5,
            },
            50,
        );

        let team_1: Vec<&dyn Combatant> = vec![&killer];
        let team_2: Vec<&dyn Combatant> = vec![&victim];
        let out = run_battle(&team_1, &team_2);

        assert!(!out.is_empty());
        let v = damage_for(&out, CombatantId::Ship(2));
        assert!(v >= 50, "victim should be finished in one round, got {v}");
    }

    #[test]
    fn one_v_one_damage_matches_formula() {
        // attack 100 vs defense 10 -> ratio 0.01, factor 0.99, floor(100 * 0.99^2) = 98
        // Health 98 ends the fight in one round with exactly that damage each side.
        let a = test_ship(
            1,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 100,
                battery_ly: 50,
                radar_ly: 5,
            },
            98,
        );
        let b = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 100,
                battery_ly: 50,
                radar_ly: 5,
            },
            98,
        );

        let team_1: Vec<&dyn Combatant> = vec![&a];
        let team_2: Vec<&dyn Combatant> = vec![&b];
        let out = run_battle(&team_1, &team_2);

        assert_eq!(damage_for(&out, CombatantId::Ship(1)), 98);
        assert_eq!(damage_for(&out, CombatantId::Ship(2)), 98);
    }

    #[test]
    fn fastest_ship_on_team_is_defender() {
        // One round: fast (higher speed) absorbs enemy fire; team attack wipes the glass enemy.
        // Slow never acts as defender, so it stays at 0 damage.
        let slow = test_ship(
            1,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 100,
                attack: 500,
                battery_ly: 50,
                radar_ly: 5,
            },
            1000,
        );
        let fast = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 10.0,
                defense: 100,
                attack: 500,
                battery_ly: 50,
                radar_ly: 5,
            },
            80,
        );
        let enemy = test_ship(
            3,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 200,
                battery_ly: 50,
                radar_ly: 5,
            },
            5,
        );

        let team_1: Vec<&dyn Combatant> = vec![&slow, &fast];
        let team_2: Vec<&dyn Combatant> = vec![&enemy];
        let out = run_battle(&team_1, &team_2);

        assert_eq!(damage_for(&out, CombatantId::Ship(1)), 0);
        assert!(damage_for(&out, CombatantId::Ship(2)) > 0);
    }

    #[test]
    fn team_attack_power_stacks() {
        // One round: 50+50 team attack equals 100 solo vs defense 100 -> 81 damage; target hp 81 ends fight.
        let left = test_ship(
            1,
            ShipStats {
                size_kt: 10,
                speed_lys: 2.0,
                defense: 50,
                attack: 50,
                battery_ly: 50,
                radar_ly: 5,
            },
            200,
        );
        let right = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 50,
                attack: 50,
                battery_ly: 50,
                radar_ly: 5,
            },
            200,
        );
        let solo = test_ship(
            3,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 50,
                attack: 100,
                battery_ly: 50,
                radar_ly: 5,
            },
            200,
        );
        let target = test_ship(
            4,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 100,
                attack: 1,
                battery_ly: 50,
                radar_ly: 5,
            },
            81,
        );

        let pair_team: Vec<&dyn Combatant> = vec![&left, &right];
        let solo_team: Vec<&dyn Combatant> = vec![&solo];
        let defenders: Vec<&dyn Combatant> = vec![&target];

        let out_pair = run_battle(&pair_team, &defenders);
        let out_solo = run_battle(&solo_team, &defenders);

        assert_eq!(damage_for(&out_pair, CombatantId::Ship(4)), 81);
        assert_eq!(damage_for(&out_solo, CombatantId::Ship(4)), 81);
    }

    #[test]
    fn stalemate_exits_without_damage() {
        let a = test_ship(
            1,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 1000,
                attack: 5,
                battery_ly: 50,
                radar_ly: 5,
            },
            100,
        );
        let b = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 1000,
                attack: 5,
                battery_ly: 50,
                radar_ly: 5,
            },
            100,
        );

        let team_1: Vec<&dyn Combatant> = vec![&a];
        let team_2: Vec<&dyn Combatant> = vec![&b];
        let out = run_battle(&team_1, &team_2);

        assert!(out.is_empty());
    }

    #[test]
    fn garrison_combatant_uses_level_stats() {
        let gar = test_garrison(1, 1, garrison_stats(1).health);
        let ship = test_ship(
            2,
            ShipStats {
                size_kt: 10,
                speed_lys: 1.0,
                defense: 10,
                attack: 1000,
                battery_ly: 50,
                radar_ly: 5,
            },
            50,
        );

        let team_1: Vec<&dyn Combatant> = vec![&gar];
        let team_2: Vec<&dyn Combatant> = vec![&ship];
        let out = run_battle(&team_1, &team_2);

        assert!(
            damage_for(&out, CombatantId::Garrison(1)) > 0
                || damage_for(&out, CombatantId::Ship(2)) > 0
        );
    }
}
