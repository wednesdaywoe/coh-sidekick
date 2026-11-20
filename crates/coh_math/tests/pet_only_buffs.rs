//! A Mastermind power that buffs only its henchmen adds nothing to the Mastermind's totals.
//!
//! Reported in the Homecoming bugs channel: Serum (Mercenaries) was applying its pet-only
//! +Damage/+ToHit/+Recovery/+Resistance/mez protection to the MM in the stat tracker. Its
//! templates are `AnyAffectedAndPets`, which the atom route reads as caster-landing
//! (`ToWho::TargetAndPets`), and the Parse7 emitter drops the `jit_requires` pet gate that
//! takes the MM back out in game. The converter now stamps `notOnCaster` on the
//! `TargetAndPets` atoms of a player power whose `targetsAffected` are all `MyPet`
//! (`stampPetOnlyBuffs`, convert-powerset.cjs), which `gather_active_atoms` drops.
//!
//! Smoke Flash (Ninjas) is the second carrier — its pet stealth radius leaked the same way.
//! Rebirth and Thunderspy ship these rows as `Target` with the gate intact and never leaked;
//! they are the control that the stamp moves nothing on the forks that were already clean.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower, ToWho};
use coh_math::totals::GlobalBonuses;
use std::path::PathBuf;

const FORKS: [DatasetId; 4] = [
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Thunderspy,
    DatasetId::Brainstorm,
];
const MERCS: &str = "mastermind/mercenaries";
const NINJAS: &str = "mastermind/ninjas";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// A level-50 Mastermind with the given primary set and one active click power, optionally
/// beside a second power in the same set. The pick is unslotted: the leak was base-value.
fn mm(db: &PowerDatabase, dataset: DatasetId, set: &str, power: Option<&str>) -> GlobalBonuses {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("mastermind".into());
    state.primary.id = Some(set.into());
    if let Some(name) = power {
        let mut pick = SelectedPower::picked(name, set, 17);
        pick.is_active = true;
        state.primary.powers = vec![pick];
    }
    let g = coh_math::recalculate(&state, db).bonuses;
    assert!(g.errors.is_empty(), "{dataset:?}: {:?}", g.errors);
    g
}

fn serum_face(g: &GlobalBonuses) -> Vec<f64> {
    vec![
        g.damage,
        g.to_hit,
        g.recovery,
        g.regeneration,
        g.resistance_smashing,
        g.resistance_lethal,
        g.resistance_fire,
        g.resistance_cold,
        g.resistance_energy,
        g.resistance_negative,
        g.resistance_psionic,
        g.resistance_toxic,
        g.protection_hold,
        g.protection_stun,
        g.protection_immobilize,
        g.protection_sleep,
        g.protection_knockback,
        g.mez_resist_hold,
        g.mez_resist_stun,
        g.mez_resist_immobilize,
        g.mez_resist_sleep,
        g.mez_resist_knockback,
    ]
}

fn assert_same(tag: &str, off: &[f64], on: &[f64]) {
    assert_eq!(off.len(), on.len());
    for (i, (a, b)) in off.iter().zip(on).enumerate() {
        assert!(
            (a - b).abs() < 1e-9,
            "{tag}: field {i} moved {a} -> {b} (off {off:?}, on {on:?})"
        );
    }
}

#[test]
fn serum_adds_nothing_to_the_masterminds_totals() {
    for dataset in FORKS {
        let db = load(dataset);
        let off = serum_face(&mm(&db, dataset, MERCS, None));
        let on = serum_face(&mm(&db, dataset, MERCS, Some("Serum")));
        assert_same(&format!("{dataset:?} Serum"), &off, &on);
    }
}

#[test]
fn smoke_flash_adds_nothing_to_the_masterminds_totals() {
    for dataset in FORKS {
        let db = load(dataset);
        let off = mm(&db, dataset, NINJAS, None);
        let on = mm(&db, dataset, NINJAS, Some("Smoke_Flash"));
        let face = |g: &GlobalBonuses| {
            vec![
                g.stealth_radius_pve,
                g.stealth_radius_pvp,
                g.damage,
                g.to_hit,
                g.recovery,
            ]
        };
        assert_same(&format!("{dataset:?} Smoke_Flash"), &face(&off), &face(&on));
    }
}

/// Stamped, not dropped: the power info panel is right to show what Serum does — only the
/// caster's sustained totals must ignore it (the Defiance precedent, appliers/damage.rs).
/// Homecoming/Brainstorm ship 28 Serum atoms and 4 Smoke Flash atoms, all `TargetAndPets`;
/// Rebirth/Thunderspy ship the rows as `Target` with the gate intact and carry none.
#[test]
fn the_pet_buff_atoms_stay_on_the_card_stamped() {
    for dataset in FORKS {
        let db = load(dataset);
        let serum = db
            .all_powers()
            .find(|p| {
                p.ident() == "Serum"
                    && p.targets_affected()
                        .unwrap_or_default()
                        .iter()
                        .all(|t| *t == "MyPet")
            })
            .expect("the mercenaries Serum");
        assert!(!serum.atoms.is_empty(), "{dataset:?}: Serum lost its atoms");
        let flash = db
            .all_powers()
            .find(|p| {
                p.ident() == "Smoke_Flash"
                    && p.targets_affected()
                        .unwrap_or_default()
                        .iter()
                        .all(|t| *t == "MyPet")
            })
            .expect("the ninjas Smoke Flash");
        for p in [serum, flash] {
            let mut landed = 0;
            for a in &p.atoms {
                if a.to_who == Some(ToWho::TargetAndPets) && a.gated != Some(true) {
                    landed += 1;
                    assert_eq!(
                        a.not_on_caster,
                        Some(true),
                        "{dataset:?}: {} carries an unstamped TargetAndPets atom",
                        p.ident()
                    );
                }
            }
            if matches!(dataset, DatasetId::Homecoming | DatasetId::Brainstorm) {
                assert!(landed > 0, "{dataset:?}: {} lost its pet rows", p.ident());
            }
        }
        if matches!(dataset, DatasetId::Homecoming | DatasetId::Brainstorm) {
            assert_eq!(serum.atoms.len(), 28, "{dataset:?}: Serum atom count moved");
        }
    }
}

/// The controls the stamp must not move: Fortify Pack's caster-landing atoms are its Self
/// charge mechanics (zero totals delta on every fork, measured 2026-11-17), and Rally the
/// Militia genuinely buffs Self+Pets — its +7.5 defense and +37.5 damage are the pre-fix
/// measured values, pinned so a stamp that over-reaches shows up here.
#[test]
fn fortify_pack_and_rally_the_militia_are_unchanged() {
    for dataset in FORKS {
        let db = load(dataset);
        let off = mm(&db, dataset, "mastermind/beast-mastery", None);
        let on = mm(
            &db,
            dataset,
            "mastermind/beast-mastery",
            Some("Fortify_Pack"),
        );
        let face = |g: &GlobalBonuses| {
            vec![
                g.defense_melee,
                g.defense_ranged,
                g.defense_aoe,
                g.defense_smashing,
                g.regeneration,
                g.damage,
                g.recovery,
            ]
        };
        assert_same(
            &format!("{dataset:?} Fortify_Pack"),
            &face(&off),
            &face(&on),
        );
    }
    let db = load(DatasetId::Thunderspy);
    let off = mm(&db, DatasetId::Thunderspy, "mastermind/knights", None);
    let on = mm(
        &db,
        DatasetId::Thunderspy,
        "mastermind/knights",
        Some("Rally_The_Militia"),
    );
    assert!(
        (on.defense_melee - off.defense_melee - 7.5).abs() < 1e-4,
        "Rally's genuine self defense moved: {} -> {}",
        off.defense_melee,
        on.defense_melee
    );
    assert!(
        (on.damage - off.damage - 37.5).abs() < 1e-4,
        "Rally's summon damage moved: {} -> {}",
        off.damage,
        on.damage
    );
}

/// A genuine self-buff on the SAME archetype still folds: the Force Field secondary's
/// Personal Force Field passive raises the MM's defense with Serum absent and present.
/// A guard that wiped every buff on a Mastermind would zero this and fail. The two
/// Homecoming forks only — the auto-passive is theirs (personal_force_field.rs).
#[test]
fn a_genuine_self_buff_still_folds_beside_serum() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        for with_serum in [false, true] {
            let mut state = CharacterState::empty(dataset);
            state.level = 50;
            state.archetype.id = Some("mastermind".into());
            state.primary.id = Some(MERCS.into());
            if with_serum {
                let mut pick = SelectedPower::picked("Serum", MERCS, 17);
                pick.is_active = true;
                state.primary.powers = vec![pick];
            }
            state.secondary.id = Some("mastermind/force-field".into());
            state.secondary.powers = vec![SelectedPower::picked(
                "Personal_Force_Field",
                "mastermind/force-field",
                1,
            )];
            let faults = coh_data::granted_powers::sync_granted_powers(&mut state, &db);
            assert!(
                faults.is_empty(),
                "{dataset:?}: grant reconcile faults: {faults:?}"
            );
            let g = coh_math::recalculate(&state, &db).bonuses;
            assert!(g.errors.is_empty(), "{dataset:?}: {:?}", g.errors);
            assert!(
                g.defense_melee > 0.0 && g.defense_ranged > 0.0 && g.defense_aoe > 0.0,
                "{dataset:?} (serum={with_serum}): the PFF passive stopped folding: {:?}",
                [g.defense_melee, g.defense_ranged, g.defense_aoe]
            );
        }
    }
}

/// The corpus tripwire: on no fork may a player power whose targets are all its own pets
/// carry an unstamped, ungated `TargetAndPets` atom — the shape that leaked Serum and
/// Smoke Flash. A future export arriving with one fails here instead of in a bug report.
#[test]
fn no_pet_only_power_carries_an_unstamped_target_and_pets_atom() {
    for dataset in FORKS {
        let db = load(dataset);
        let mut offenders = Vec::new();
        for p in db.all_powers() {
            let ta = p.targets_affected().unwrap_or_default();
            if ta.is_empty() || !ta.iter().all(|t| *t == "MyPet") {
                continue;
            }
            for a in &p.atoms {
                if a.to_who == Some(ToWho::TargetAndPets)
                    && a.gated != Some(true)
                    && a.not_on_caster != Some(true)
                {
                    offenders.push(p.ident().to_string());
                    break;
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "{dataset:?}: pet-only powers with unstamped TargetAndPets atoms: {offenders:?}"
        );
    }
}
