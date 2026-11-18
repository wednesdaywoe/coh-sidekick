//! The henchman projection (worklog henchman-panel-1): the roster a Mercenaries Mastermind fields
//! and each henchman's own stats, pinned to the committed probe's Homecoming numbers at L50
//! pet-class tables. The roster derives from the summon powers' `EntCreate` atoms — the additive
//! `entity_def` wire field plus the `combatlevel` gates — so the Soldiers Medic (the gated third
//! slot) is named and counted, which the collapsed summon bag could not do.
//!
//! The damage headline counts EVERY damage-dealing henchman, Medic included (owner ruled
//! 2026-11-17): the Medic fires SMG_Burst at the same proxy as a Soldier, so the faithful
//! fully-upgraded headline is 23.26 dmg/s, not the 21.0 that wrongly excluded it.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::henchmen::{buff_delta, buffed_pet, pet_detail, power_roster, project, project_power, roster};
use std::path::PathBuf;

const MERCS: &str = "mastermind/mercenaries";
const UPGRADES: [&str; 2] = ["Equip_Mercenary", "Tactical_Upgrade"];

fn load() -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract/homecoming/bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load homecoming: {e}"))
}

/// A Mercenaries Mastermind at `level` with the summon powers, both upgrades, and Serum active.
fn merc_build(level: u8) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = level;
    state.archetype.id = Some("mastermind".into());
    state.primary.id = Some(MERCS.into());
    for name in ["Soldiers", "Spec_Ops", "Commando", "Equip_Mercenary", "Tactical_Upgrade", "Serum"] {
        let mut pick = SelectedPower::picked(name, MERCS, level);
        pick.is_active = true;
        state.primary.powers.push(pick);
    }
    state
}

/// A build with only Soldiers picked, to isolate its two level gates (>5 second Soldier, >17 Medic).
fn soldiers_build(level: u8) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = level;
    state.archetype.id = Some("mastermind".into());
    state.primary.id = Some(MERCS.into());
    state.primary.powers = vec![SelectedPower::picked("Soldiers", MERCS, level)];
    state
}

fn count(roster: &[coh_math::henchmen::RosterEntry], entity: &str) -> f64 {
    roster
        .iter()
        .find(|e| e.entity == entity)
        .map(|e| e.count)
        .unwrap_or(0.0)
}

#[test]
fn a_soldiers_own_resistance_is_26_percent_smashing_lethal() {
    let db = load();
    let mut errors = vec![];
    let detail = pet_detail(&db, "MastermindPets_Soldier", 50, 1.0, &|_| false, &mut errors).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(detail.class, "henchman_minion");
    for ty in ["smashing", "lethal"] {
        let r = detail.survivability.resistance[ty];
        assert!((r - 0.261).abs() < 1e-3, "{ty} resistance {r} != 0.261");
    }
}

#[test]
fn quick_burst_is_6_74_damage_over_a_3_second_recharge() {
    let db = load();
    let mut errors = vec![];
    let detail = pet_detail(&db, "MastermindPets_Soldier", 50, 1.0, &|_| false, &mut errors).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let burst = detail
        .attacks
        .iter()
        .find(|a| a.internal_name == "ASMG_Burst")
        .expect("Soldier has Quick Burst (ASMG_Burst)");
    assert!((burst.damage_per_activation - 6.74).abs() < 0.01, "{}", burst.damage_per_activation);
    assert!((burst.recharge - 3.0).abs() < 1e-6);
    assert!((burst.proxy_dps - 2.25).abs() < 0.01, "{}", burst.proxy_dps);
}

#[test]
fn the_soldiers_roster_follows_its_level_gates() {
    let db = load();
    // L50: both gates pass — 2 Soldiers + 1 Medic (the help: "the third Soldier you gain will be a Medic").
    let r50 = roster(&db, &soldiers_build(50));
    assert_eq!(count(&r50, "MastermindPets_Soldier"), 2.0);
    assert_eq!(count(&r50, "MastermindPets_Medic"), 1.0);
    // L10: the >5 gate passes but >17 does not — 2 Soldiers, no Medic yet.
    let r10 = roster(&db, &soldiers_build(10));
    assert_eq!(count(&r10, "MastermindPets_Soldier"), 2.0);
    assert_eq!(count(&r10, "MastermindPets_Medic"), 0.0);
    // L1: neither gate passes — the single base Soldier only.
    let r1 = roster(&db, &soldiers_build(1));
    assert_eq!(count(&r1, "MastermindPets_Soldier"), 1.0);
    assert_eq!(count(&r1, "MastermindPets_Medic"), 0.0);
}

#[test]
fn the_headline_counts_every_damage_henchman_including_the_medic() {
    let db = load();
    let proj = project(&db, &merc_build(50), &|n| UPGRADES.contains(&n));
    assert!(proj.errors.is_empty(), "{:?}", proj.errors);
    // Roster: 2 Soldiers + 1 Medic + 2 Spec Ops + 1 Commando.
    assert_eq!(count(&proj.roster, "MastermindPets_Soldier"), 2.0);
    assert_eq!(count(&proj.roster, "MastermindPets_Medic"), 1.0);
    assert_eq!(count(&proj.roster, "MastermindPets_Spec_Ops"), 2.0);
    assert_eq!(count(&proj.roster, "MastermindPets_Commando"), 1.0);
    // The Medic is a damage henchman: its SMG_Burst proxy matches a Soldier's Quick Burst.
    let medic = proj.pets.iter().find(|p| p.entity == "MastermindPets_Medic").unwrap();
    assert!((medic.best_proxy - 2.25).abs() < 0.01, "{}", medic.best_proxy);
    // Fully-upgraded faithful headline: Soldier 4.50 + Spec_Ops 12.38 + Commando 4.13 + Medic 2.25.
    assert!((proj.headline_dps - 23.26).abs() < 0.05, "{}", proj.headline_dps);
}

#[test]
fn serum_buffs_the_pet_and_the_buffed_state_only_rises() {
    let db = load();
    let state = merc_build(50);
    let mut errors = vec![];
    let delta = buff_delta(&db, &state, "henchman_minion", 50, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(delta.resistance["smashing"] > 0.0, "{:?}", delta.resistance);
    assert!(delta.damage["lethal"] > 0.0, "{:?}", delta.damage);
    assert!(delta.to_hit > 0.0);
    assert!(delta.recovery > 0.0);

    let base = pet_detail(&db, "MastermindPets_Soldier", 50, 1.0, &|_| false, &mut errors).unwrap();
    let buffed = buffed_pet(&base, &delta);
    assert!(
        buffed.survivability.resistance["smashing"] > base.survivability.resistance["smashing"],
        "buffed resistance must rise"
    );
    assert!(buffed.best_proxy >= base.best_proxy, "buffed damage must not fall");
    // The base detail is untouched by the opt-in buff.
    assert!((base.survivability.resistance["smashing"] - 0.261).abs() < 1e-3);
}

#[test]
fn a_summon_powers_roster_and_headline_are_its_own_pets() {
    let db = load();
    // The Info panel reads ONE summon power: Soldiers fields 2 Soldiers + 1 Medic, nobody else.
    let soldiers = db.find_power(MERCS, "Soldiers").unwrap();
    let proj = project_power(&db, soldiers, 50, &|n| UPGRADES.contains(&n));
    assert!(proj.errors.is_empty(), "{:?}", proj.errors);
    assert_eq!(proj.roster.len(), 2);
    assert_eq!(count(&proj.roster, "MastermindPets_Soldier"), 2.0);
    assert_eq!(count(&proj.roster, "MastermindPets_Medic"), 1.0);
    // HC: Soldier 2.25 x2 + Medic 2.25 = 6.75, not the whole-build 23.26.
    assert!((proj.headline_dps - 6.75).abs() < 0.05, "{}", proj.headline_dps);
    // Spec Ops is its own roster of 2, with no Soldier/Medic bleed.
    let spec = db.find_power(MERCS, "Spec_Ops").unwrap();
    let r = power_roster(&db, spec, 50);
    assert_eq!(r.len(), 1);
    assert_eq!(count(&r, "MastermindPets_Spec_Ops"), 2.0);
}
