//! A mob's `CombatTracker`: the hits it took lately, which its death message
//! is built from (`TamableAnimal.die` sends it to the owner).
use super::{Damage, Mob, MobWorld};

/// `CombatEntry`: one hit, with where the mob was falling from when it landed.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub source: Damage,
    pub damage: f32,
    /// `FallLocation.getCurrentFallLocation` is `WATER`. Horizon keeps no
    /// `lastClimbablePos` for mobs, so the climbable locations are not known.
    pub in_water: bool,
    pub fall_distance: f32,
}

/// `CombatTracker` for a mob, timed by its `tickCount` (`Mob::age`).
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    pub entries: Vec<Entry>,
    last_damage_time: u32,
    pub in_combat: bool,
    taking_damage: bool,
}

impl Tracker {
    /// `CombatTracker.recordDamage`: the hit after forgetting an old fight; a
    /// living cause (`shouldEnterCombat`) starts a fight. It runs before the
    /// hit takes the health (`actuallyHurt`), so the mob is still alive.
    pub fn record(&mut self, entry: Entry, tick: u32) {
        self.recheck(true, tick);
        let living_cause = entry.source.attacker.is_some();
        self.entries.push(entry);
        self.last_damage_time = tick;
        self.taking_damage = true;
        if !self.in_combat && living_cause {
            self.in_combat = true;
        }
    }
    /// `CombatTracker.recheckStatus`: 100 ticks without a hit (300 in a
    /// fight) forget them all. Vanilla also forgets them once the mob is
    /// dead, after `die` has read them; Horizon's `die` runs after the tick,
    /// so a dead mob's hits stay for it.
    pub fn recheck(&mut self, alive: bool, tick: u32) {
        let reset = if self.in_combat { 300 } else { 100 };
        if alive && self.taking_damage && tick.wrapping_sub(self.last_damage_time) > reset {
            self.taking_damage = false;
            self.in_combat = false;
            self.entries.clear();
        }
    }
}

/// `setLastHurtByMob(null)` for a resolved attacker that is dead or whose
/// timestamp is more than 100 ticks old; it also resets the timestamp.
pub(super) fn expire(m: &mut Mob, w: &MobWorld) {
    if let Some(attacker) = m.ai.last_hurt_by.and_then(|who| w.target_info(who))
        && (!attacker.alive || (m.age as i32).wrapping_sub(m.ai.last_hurt_by_time) > 100)
    {
        m.ai.last_hurt_by = None;
        m.ai.last_hurt_by_time = m.age as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::{DamageKind, Who};

    fn hit(attacker: Option<Who>) -> Entry {
        Entry {
            source: Damage {
                kind: DamageKind::MobAttack,
                attacker,
                from: None,
                attacker_species: None,
            },
            damage: 2.0,
            in_water: false,
            fall_distance: 0.0,
        }
    }

    #[test]
    fn old_hits_are_forgotten_after_100_ticks_or_300_in_a_fight() {
        let mut t = Tracker::default();
        t.record(hit(None), 10);
        t.recheck(true, 110);
        assert_eq!(t.entries.len(), 1, "100 ticks later it still remembers");
        t.recheck(true, 111);
        assert!(t.entries.is_empty());
        t.record(hit(Some(Who::Mob(7))), 200);
        assert!(t.in_combat);
        t.recheck(true, 500);
        assert_eq!(t.entries.len(), 1, "a fight is remembered for 300 ticks");
        t.record(hit(None), 501);
        assert_eq!(t.entries.len(), 1, "the new hit comes after forgetting");
        assert!(!t.in_combat);
    }

    #[test]
    fn a_dead_mob_keeps_its_hits_for_its_death_message() {
        let mut t = Tracker::default();
        t.record(hit(Some(Who::Mob(7))), 0);
        t.recheck(false, 1000);
        assert_eq!(t.entries.len(), 1);
    }
}

#[cfg(test)]
mod last_hurt_by_tests {
    use super::super::{Species, Who};
    use super::*;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    #[test]
    fn last_attacker_expires_after_100_ticks_or_on_death() {
        crate::mobs::world::with_test_world(&[], |world, _| {
            let mut world = world.clone();
            world.player = Some(crate::mobs::world::test_player(Vec3::ZERO));
            let mut t = (*crate::mobs::sensor_scan_tests::kind()).clone();
            t.species = Species::Fox;
            let mut fox = Mob::new(1, Arc::new(t), Vec3::new(0.0, 64.0, 0.0), 0.0);
            fox.ai.last_hurt_by = Some(Who::Player);
            fox.ai.last_hurt_by_time = 10;
            fox.age = 110;
            expire(&mut fox, &world);
            assert_eq!(fox.ai.last_hurt_by, Some(Who::Player));
            assert_eq!(fox.ai.last_hurt_by_time, 10);
            fox.age = 111;
            expire(&mut fox, &world);
            assert_eq!(fox.ai.last_hurt_by, None);
            assert_eq!(fox.ai.last_hurt_by_time, 111);

            fox.ai.last_hurt_by = Some(Who::Player);
            fox.ai.last_hurt_by_time = 111;
            fox.age = 112;
            world.player.as_mut().unwrap().alive = false;
            expire(&mut fox, &world);
            assert_eq!(fox.ai.last_hurt_by, None);
            assert_eq!(fox.ai.last_hurt_by_time, 112);
        });
    }
}
