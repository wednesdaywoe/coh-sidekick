//! Auto/toggle proc chances score the host power's area factor, not a blanket single target.
//!
//! An auto or toggle's check CLOCK is the proc piece's own activate period (PPM-1), but the
//! roll still happens against every foe its area reaches. The two legs here pin that split:
//! a 15ft Entangling Aura takes the AoE penalty, a radius-0 Stamina does not.

use coh_data::{CharacterState, DatasetId, Enhancement, PowerDatabase, SelectedPower};
use coh_math::procs::{slotted_procs, ProcRoll};
use std::path::PathBuf;

fn load() -> PowerDatabase {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/homecoming/bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("load homecoming")
}

fn proc(set_id: &str, set_name: &str, io_name: &str) -> Enhancement {
    serde_json::from_value(serde_json::json!({
        "type": "io-set", "id": "e1", "name": io_name, "icon": "",
        "level": 50, "attuned": false, "boost": 0, "set_id": set_id,
        "set_name": set_name, "piece_num": 6, "aspects": [],
        "is_proc": true, "is_unique": false
    }))
    .expect("slot shape")
}

/// The PerCheck row for one slotted piece, or the whole ProcRoll if it scored differently.
fn check(powerset: &str, power: &str, proc: Enhancement) -> ProcRoll {
    let db = load();
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = 50;
    state.archetype.id = Some("defender".to_string());
    let mut pick = SelectedPower::picked(power, powerset, 1);
    pick.slots = vec![Some(proc)];
    state.primary.powers = vec![pick];
    let rows = slotted_procs(&state, &db, powerset, power);
    assert_eq!(rows.len(), 1, "{power}: expected one proc row");
    rows[0].roll.clone()
}

fn close(ours: f64, theirs: f64, what: &str) {
    assert!(
        (ours - theirs).abs() < 1e-9,
        "{what}: ours {ours}, expected {theirs}"
    );
}

#[test]
fn entangling_aura_absorb_proc_reads_18_6_percent() {
    // Superior Entomb's +Absorb is 3 PPM on the proc's 10s period. Entangling Aura is a 15ft
    // toggle, so the area denominator is 0.25 + 0.75 × (1 + 15 × 0.15) = 2.6875:
    // 3 × 10 / (60 × 2.6875) = 18.6047%. Forcing single target would give 50%.
    let roll = check(
        "defender/nature-affinity",
        "Raging_Tempest",
        proc("superior_entomb", "Superior Entomb", "Chance for +Absorb"),
    );
    let (ppm, chance, period, area_factor, ppm_mod) = match roll {
        ProcRoll::PerCheck {
            ppm,
            chance,
            period,
            area_factor,
            ppm_mod,
        } => (ppm, chance, period, area_factor, ppm_mod),
        ref other => panic!("expected a per-check roll, got {other:?}"),
    };
    close(ppm, 3.0, "ppm");
    close(period, 10.0, "check period");
    close(ppm_mod, 1.0, "ppm mod");
    close(area_factor, 2.6875, "area denominator");
    close(chance, 3.0 * 10.0 / (60.0 * 2.6875), "chance per check");
    close(chance, 0.18604651162790697, "the reported 18.6%");
}

#[test]
fn single_target_toggle_proc_keeps_its_denominator() {
    // Performance Shifter's +End is 1.5 PPM on its own 10s period. Stamina is an auto with no
    // radius, so the area denominator stays 1.0 and the chance stays 1.5 × 10 / 60 = 25%.
    let roll = check(
        "fitness",
        "Stamina",
        proc(
            "performance_shifter",
            "Performance Shifter",
            "Chance for Endurance Buff",
        ),
    );
    let (ppm, chance, period, area_factor, ppm_mod) = match roll {
        ProcRoll::PerCheck {
            ppm,
            chance,
            period,
            area_factor,
            ppm_mod,
        } => (ppm, chance, period, area_factor, ppm_mod),
        ref other => panic!("expected a per-check roll, got {other:?}"),
    };
    close(ppm, 1.5, "ppm");
    close(period, 10.0, "check period");
    close(ppm_mod, 1.0, "ppm mod");
    close(area_factor, 1.0, "single-target area denominator");
    close(chance, 0.25, "chance per check");
}
