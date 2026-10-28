//! A power that deals its damage through a summoned pseudo-pet must show that damage.
//!
//! Sonic Boom, Lightning Rod, Shield Charge and the rains carry no damage atom on the parent: the
//! hit lives on the pet, so the card read them as non-attacks. Each pet's own atoms are now read
//! through the same resolver a direct attack uses, counted over the hits one summon lands.
//!
//! The other half is what must NOT fold. A pet that follows its owner between zones (Summon
//! Widow, Phantom Army's decoys) fights beside the player; its 240-second lifetime read as an
//! 18,605-damage "hit" before `canZone` kept it out. And a patch that exists only once something
//! ignites it (Oil Slick Arrow's fire) is inert until it does.

use coh_data::{DatasetId, Power, PowerDatabase};
use coh_math::damage::{resolve_power_damage, DamageApplication, PowerDamage};
use coh_math::expr::{SourceContext, TargetIdentity};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn damage(db: &PowerDatabase, archetype: &str, power: &Power) -> PowerDamage {
    let ctx = SourceContext {
        target: Some(TargetIdentity {
            archetype_class: None,
            entity_type: "critter".to_string(),
            modes: Default::default(),
            held: false,
        }),
        caster_class: db.class_name_of(archetype).map(str::to_string),
        ..Default::default()
    };
    let mut errors = Vec::new();
    let damage = resolve_power_damage(power, archetype, 50, 0.0, 0.0, &ctx, db, &mut errors);
    assert!(errors.is_empty(), "{}: {errors:?}", power.ident());
    damage
}

/// Every power in the dataset with this identity — one per archetype that has it.
fn powers<'a>(db: &'a PowerDatabase, ident: &str) -> Vec<&'a Power> {
    let found: Vec<&Power> = db.all_powers().filter(|p| p.ident() == ident).collect();
    assert!(!found.is_empty(), "no power {ident}");
    found
}

fn table(db: &PowerDatabase, archetype: &str, name: &str) -> f64 {
    db.at_tables
        .get_table_value(archetype, name, 50)
        .unwrap_or_else(|| panic!("{archetype} has no {name}"))
        .abs()
}

#[test]
fn sonic_boom_hits_once_through_its_pseudo_pet() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        for power in powers(&db, "Sonic_Boom") {
            let damage = damage(&db, "brute", power);
            let [component] = damage.components.as_slice() else {
                panic!("{dataset:?}: {:?}", damage.components);
            };
            let summoned = component.summoned.as_ref().expect("dealt by the pet");
            // The pet pulses every 75s and lives 4s: the hit at spawn is the only one.
            assert_eq!(summoned.fires, 1.0);
            assert_eq!(component.damage_type, "Energy");
            let expected = 0.933 * table(&db, "brute", "Melee_Damage");
            assert!(
                (damage.base - expected).abs() < 0.01,
                "{dataset:?}: {} against {expected}",
                damage.base
            );
        }
    }
}

#[test]
fn every_teleport_attack_has_its_damage() {
    let db = load(DatasetId::Homecoming);
    for ident in ["Sonic_Boom", "Lightning_Rod", "Shield_Charge"] {
        for power in powers(&db, ident) {
            let damage = damage(&db, "brute", power);
            assert!(damage.base > 0.0, "{ident} shows no damage");
        }
    }
}

#[test]
fn a_rain_lands_every_pulse_of_its_life() {
    let db = load(DatasetId::Homecoming);
    let power = powers(&db, "Rain_of_Fire")
        .into_iter()
        .find(|p| {
            p.summon()
                .and_then(|s| s.get("entity"))
                .and_then(|e| e.as_str())
                == Some("Pets_RainofFire")
        })
        .expect("an entity-table Rain of Fire");
    let damage = damage(&db, "blaster", power);
    let component = damage
        .components
        .iter()
        .find(|c| c.is_certain())
        .expect("a certain component");
    let summoned = component.summoned.as_ref().expect("dealt by the pet");
    // 0.2s pulses over 15s, the pulse at spawn included.
    assert_eq!(summoned.fires, 76.0);
    assert!((component.total.base - component.base * 76.0).abs() < 1e-9);
}

#[test]
fn a_pet_that_follows_its_owner_is_not_the_powers_hit() {
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        for ident in ["Summon_Widow", "Decoy", "Gang_War"] {
            for power in db.all_powers().filter(|p| p.ident() == ident) {
                let damage = damage(&db, "controller", power);
                assert!(
                    damage.components.iter().all(|c| c.summoned.is_none()),
                    "{dataset:?} {ident}: {:?}",
                    damage.components
                );
            }
        }
    }
}

#[test]
fn an_unignited_patch_is_inert() {
    let db = load(DatasetId::Homecoming);
    for power in powers(&db, "Oil_Slick_Arrow") {
        let damage = damage(&db, "corruptor", power);
        let burns: Vec<_> = damage
            .components
            .iter()
            .filter(|c| c.summoned.as_ref().is_some_and(|s| s.condition.is_some()))
            .collect();
        assert!(!burns.is_empty(), "the burn is shown");
        assert!(burns
            .iter()
            .all(|c| c.application == DamageApplication::Dormant));
        assert_eq!(damage.base, 0.0, "nothing ignited it");
    }
}

#[test]
fn a_power_with_its_own_hit_keeps_it() {
    let db = load(DatasetId::Homecoming);
    // Only the ones that ARE the blast: the Dominator epic's `Fire_Blast` is Rain of Fire under
    // another internal name, and a rain is exactly what should fold.
    for power in powers(&db, "Fire_Blast") {
        if !power
            .atoms
            .iter()
            .any(|atom| atom.effect_type == Some(coh_data::EffectType::Damage))
        {
            continue;
        }
        let damage = damage(&db, "blaster", power);
        assert!(damage.components.iter().all(|c| c.summoned.is_none()));
    }
}

/// Meteor's shell places a second shell that runs the hit, and a Trip Mine's self-destruct fires
/// the hit as a power of its own. The pet's atoms stopped a hop short of both, so each attack
/// showed no damage at all. Homecoming's only: the other forks state these pets differently.
#[test]
fn a_hit_one_hop_further_on_still_lands() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        let meteor = db
            .find_power("blaster/seismic-blast", "Meteor")
            .expect("Blaster Meteor");
        let meteor = damage(&db, "blaster", meteor);
        // 1.0 Fire and 3.0 Smashing, from one meteor.
        let expected = 4.0 * table(&db, "blaster", "Ranged_Damage");
        assert!(
            (meteor.base - expected).abs() < 0.01,
            "{dataset:?} Meteor: {} against {expected}",
            meteor.base
        );

        let mut mines = 0;
        for set in db.powersets.iter() {
            let Some(mine) = set.powers.iter().find(|p| p.ident() == "Trip_Mine") else {
                continue;
            };
            let archetype = set.archetype.as_deref().expect("an archetype set");
            let damage = damage(&db, archetype, mine);
            assert!(damage.base > 0.0, "{dataset:?} {}: no damage", set.id);
            // A mine goes off once, however long it waits: the Dominator's names its
            // self-destruct differently, and read nine detonations over its 170s.
            for component in &damage.components {
                let fires = component.summoned.as_ref().map(|s| s.fires);
                assert_eq!(fires, Some(1.0), "{dataset:?} {}", set.id);
            }
            mines += 1;
        }
        assert_eq!(mines, 6, "{dataset:?}: every archetype's Trip Mine");
    }
}

/// The epic converter built the summon but never read what a placed shell does, so epic Bonfire
/// and the Dominator's Sleet came through as pets with no payload.
#[test]
fn an_epic_patch_has_its_damage() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        for (set, ident) in [
            ("flame_mastery", "Bonfire"),
            ("heat_mastery_stalker", "Bonfire"),
            ("ice_mastery_dominator", "Ice_Blast"),
        ] {
            let power = db
                .epic_powers
                .iter()
                .find(|p| p.set_id == set && p.power.ident() == ident)
                .unwrap_or_else(|| panic!("{dataset:?}: no {set} {ident}"));
            let damage = damage(&db, "blaster", &power.power);
            assert!(damage.base > 0.0, "{dataset:?} {set} {ident}: no damage");
        }
    }
}
