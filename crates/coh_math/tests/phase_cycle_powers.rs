//! A two-click PHASE power shows its whole cycle on the card — the `phase-cycle-powers` pins.
//!
//! THE REPORT (2026-11-10): on Homecoming, Temporal Bomb (the reworked Time Bomb) showed no
//! damage and no recharge on its Info card, and its buff/debuff numbers appeared in Totals but
//! nowhere on the card. Time Bomb on Rebirth/Thunderspy showed everything.
//!
//! THE CAUSE: the rework made the player record a redirect shell over a two-click cycle —
//! click 1 places or attaches the bomb (`kRemoteBomb Mode? !`), click 2 detonates it
//! (`kRemoteBomb Mode?`), and a never-firing `Remote_Bomb_Info` record carries the payload for
//! display. `extractFormVariants` claimed the table, so the engine's first-match walk swapped
//! the whole record to the PLACE phase: the projection resolved the place branch's 2.77s cast
//! and 26 endurance and nothing else — zero damage components, zero effect rows. The payload
//! was never lost: the base record's union atoms already carried the detonation damage, the
//! Knockback and the Temporal Bubble rows, each with its own branch gate. Only the swap hid
//! them, and no branch left standing stated the recharge — the 150s (traps) / 360s (blaster)
//! lives on the detonate branch's stats.
//!
//! THE FIX is converter-side (`phaseCycleMode` / `stampPhaseCycleStats`): the phase table is
//! declined for formVariants — the mode its complementary pair selects on is published through
//! the power's OWN grant chain (the bomb marker's `Set_Mode RemoteBomb`), which is what makes
//! the branches phases of one cycle rather than alternatives — and the executing branches'
//! stats are stamped onto the base record. The engine is unchanged: with no formVariants the
//! base record projects whole, which IS the detonation view the card should show.
//!
//! These pins grade the engine on the regenerated Homecoming and Brainstorm bundles, and hold
//! the regen's shape: only the 8 Time Bomb powers moved, the Kheldian form pairs (claimed by
//! `extractModeVariants`, whose modes the shapeshift powers set) and Energy Transfer's pair
//! (formVariants, an owned-charge spender) did not.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn held(dataset: DatasetId, archetype: &str, set: &str) -> (PowerDatabase, CharacterState) {
    let db = load(dataset);
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some(archetype.into());
    state.primary.powers = vec![SelectedPower::picked("Time_Bomb", set, 1)];
    (db, state)
}

fn bomb(totals: &coh_math::CalculatedTotals) -> &coh_math::projection::PowerProjection {
    totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Time_Bomb")
        .expect("Time_Bomb projected")
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-2,
        "expected {expected}, got {actual}"
    );
}

fn row<'a>(
    projection: &'a coh_math::projection::PowerProjection,
    key: &str,
) -> &'a coh_math::granted::GrantedMagnitude {
    projection
        .granted_magnitudes
        .iter()
        .find(|m| m.row_key == key)
        .unwrap_or_else(|| panic!("no {key} row among {:?}", projection.granted_magnitudes))
}

/// The Temporal Bomb card is the detonation: the hit, the Knockback, the Temporal Bubble, and
/// the whole cycle's costs — 150s recharge, 2.77s cast, 26 endurance — none of which the card
/// had before the converter stopped swapping the record to the place phase.
#[test]
fn the_temporal_bomb_card_is_the_detonation() {
    let (db, state) = held(DatasetId::Homecoming, "controller", "controller/traps");
    let totals = coh_math::recalculate(&state, &db);
    let projection = bomb(&totals);

    // The def ships no formVariants and carries the cycle's stats on the base record.
    let def = db.find_power("controller/traps", "Time_Bomb").unwrap();
    assert!(
        def.extra.get("formVariants").is_none(),
        "the phase table still ships as a form swap"
    );
    let stats = def.extra.get("stats").expect("stats block");
    for (key, value) in [
        ("recharge", 150.0),
        ("endurance", 26.0),
        ("castTime", 2.77),
        ("accuracy", 2.0),
    ] {
        near(
            stats
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(f64::NAN),
            value,
        );
    }

    // The cycle's costs, projected.
    near(projection.recharge.as_ref().expect("recharge").base, 150.0);
    near(projection.cast_time.as_ref().expect("cast").base, 2.77);
    near(
        projection.endurance_cost.as_ref().expect("endurance").base,
        26.0,
    );

    // The detonation hit: extreme Lethal/Fire, one row each, both certain.
    near(projection.damage.base, 61.17);
    assert_eq!(projection.damage.components.len(), 2, "two components");
    for (type_name, expected) in [("Fire", 36.15), ("Lethal", 25.02)] {
        let component = projection
            .damage
            .components
            .iter()
            .find(|c| c.damage_type == type_name)
            .unwrap_or_else(|| panic!("no {type_name} component"));
        near(component.total.base, expected);
    }

    // The Temporal Bubble and the blast's mez, as rows the card renders.
    near(row(projection, "knockback").value.base, 8.31);
    near(row(projection, "slow").value.base, 50.0);
    near(row(projection, "rechargeDebuff").value.base, 40.0);
    near(row(projection, "recoveryDebuff").value.base, 30.0);
    near(row(projection, "rechargeBuff").value.base, 37.5);
    near(row(projection, "recoveryBuff").value.base, 37.5);
    near(row(projection, "buffDuration").value.base, 30.0);
}

/// The Blaster Devices copy (Remote Bomb) is its own record: a harder hit, and the detonate
/// branch's own 360s recharge — stamped from the branches the same way, never guessed.
#[test]
fn the_blaster_bomb_is_its_own_record() {
    let (db, state) = held(DatasetId::Homecoming, "blaster", "blaster/devices");
    let totals = coh_math::recalculate(&state, &db);
    let projection = bomb(&totals);

    near(projection.recharge.as_ref().expect("recharge").base, 360.0);
    near(projection.damage.base, 278.06);
    for (type_name, expected) in [("Fire", 111.22), ("Lethal", 166.84)] {
        let component = projection
            .damage
            .components
            .iter()
            .find(|c| c.damage_type == type_name)
            .unwrap_or_else(|| panic!("no {type_name} component"));
        near(component.total.base, expected);
    }
    let def = db.find_power("blaster/devices", "Time_Bomb").unwrap();
    assert!(def.extra.get("formVariants").is_none());
}

/// Brainstorm ships the same rework; its bundles move with the same rule and the same numbers.
#[test]
fn brainstorm_ships_the_same_cycle() {
    let db = load(DatasetId::Brainstorm);
    let def = db.find_power("controller/traps", "Time_Bomb").unwrap();
    assert!(def.extra.get("formVariants").is_none());
    let stats = def.extra.get("stats").expect("stats block");
    for (key, value) in [("recharge", 150.0), ("endurance", 26.0), ("castTime", 2.77)] {
        near(
            stats
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(f64::NAN),
            value,
        );
    }
}

/// The forks that never reworked the power are untouched: Rebirth's Time Bomb projects whole
/// through its summon, as it did before the phase rule existed.
#[test]
fn rebirth_time_bomb_is_unchanged() {
    let (db, state) = held(DatasetId::Rebirth, "controller", "controller/traps");
    let totals = coh_math::recalculate(&state, &db);
    let projection = bomb(&totals);

    near(projection.recharge.as_ref().expect("recharge").base, 360.0);
    near(projection.cast_time.as_ref().expect("cast").base, 9.0);
    near(
        projection.endurance_cost.as_ref().expect("endurance").base,
        26.0,
    );
    near(projection.damage.base, 278.05);
    assert_eq!(projection.damage.components.len(), 2);
}

/// The phase rule claims the Time Bomb tables and nothing else. The two populations a
/// stranger would expect it to swallow are pinned: Energy Transfer's complementary pair is
/// still a form swap (its mode is an OWNED CHARGE, not a phase the power grants), and the
/// Kheldian form pair is still a mode variant (its mode is the shapeshift's, set elsewhere).
#[test]
fn the_form_detectors_still_serve_their_tables() {
    let db = load(DatasetId::Homecoming);
    let transfer = db
        .find_power("brute/energy-melee", "Energy_Transfer")
        .unwrap();
    assert!(
        transfer.extra.get("formVariants").is_some(),
        "Energy Transfer lost its formVariants"
    );
    let gleaming = db
        .find_power("peacebringer/luminous-blast", "Gleaming_Bolt")
        .unwrap();
    assert!(
        gleaming.extra.get("modeVariants").is_some(),
        "Gleaming Bolt lost its modeVariants"
    );
}
