//! Radiance (`Mirror_Image`) unlocks a bonus on every other Light Affinity power, and none of them
//! saw it. Three gaps, each enough on its own:
//!
//! * Radiance sets its mode, `Chain_Jolt_Mode`, from its activation effects, which the mode
//!   collector never read, so a build running Radiance published no mode at all.
//! * Blinding Beam's bonus (-ToHit, -Damage) sits in `Refracted_BlindingBeam`, the power it fires,
//!   and the converter never followed a fired power for its gated groups.
//! * A bonus toggle stood for the mode but answered only to its own switch, so running Radiance
//!   moved the totals and left every Info panel unchanged.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::projection::PowerProjection;
use std::path::PathBuf;

/// The forks that ship Light Affinity.
// dataset-absent: rebirth, thunderspy — neither ships Light Affinity.
const DATASETS: &[DatasetId] = &[DatasetId::Homecoming, DatasetId::Brainstorm];

const SET: &str = "defender/light-affinity";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// A level-50 Defender holding Radiance and three of the powers it powers up.
fn project(dataset: DatasetId, db: &PowerDatabase, radiance: bool) -> Vec<PowerProjection> {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("defender".into());
    state.primary.id = Some(SET.into());
    state.primary.powers = [
        "Mirror_Image",
        "Blinding_Beam",
        "Spotlight",
        "Searing_Beam",
        "Protective_Beam",
    ]
    .into_iter()
    .map(|pick| {
        assert!(
            db.resolve_power(SET, pick).is_some(),
            "{dataset:?}: no {pick}"
        );
        let mut power = SelectedPower::picked(pick, SET, 1);
        power.is_active = pick == "Mirror_Image" && radiance;
        power
    })
    .collect();
    coh_data::sync_granted_powers(&mut state, db);
    coh_math::recalculate(&state, db).power_projection
}

fn shows(projected: &[PowerProjection], power: &str, key: &str) -> bool {
    projected
        .iter()
        .find(|p| p.power_internal_name == power)
        .unwrap_or_else(|| panic!("{power} projected"))
        .granted_magnitudes
        .iter()
        .any(|row| row.effect_key == key)
}

/// The base value of `power`'s `key` row, which must exist.
fn row(projected: &[PowerProjection], power: &str, key: &str) -> f64 {
    projected
        .iter()
        .find(|p| p.power_internal_name == power)
        .unwrap_or_else(|| panic!("{power} projected"))
        .granted_magnitudes
        .iter()
        .find(|row| row.effect_key == key)
        .unwrap_or_else(|| panic!("{power} has no {key} row"))
        .value
        .base
}

fn damage(projected: &[PowerProjection], power: &str) -> f64 {
    projected
        .iter()
        .find(|p| p.power_internal_name == power)
        .unwrap_or_else(|| panic!("{power} projected"))
        .damage
        .base
}

#[test]
fn radiance_unlocks_the_light_affinity_bonuses() {
    for &dataset in DATASETS {
        let db = load(dataset);
        let off = project(dataset, &db, false);
        let on = project(dataset, &db, true);

        assert!(!shows(&off, "Blinding_Beam", "damageDebuff"));
        assert!(
            shows(&on, "Blinding_Beam", "damageDebuff"),
            "{dataset:?}: Blinding Beam's Radiance -Damage is missing"
        );
        assert!(!shows(&off, "Spotlight", "damageBuff"));
        assert!(
            shows(&on, "Spotlight", "damageBuff"),
            "{dataset:?}: Spotlight's Radiance +Damage is missing"
        );
        // A second -ToHit and a second absorb stand beside the base ones, so the rows sum.
        let tohit = (
            row(&off, "Blinding_Beam", "tohitDebuff"),
            row(&on, "Blinding_Beam", "tohitDebuff"),
        );
        assert!(
            (tohit.1 - 2.0 * tohit.0).abs() < 1e-6,
            "{dataset:?}: Blinding Beam's -ToHit {tohit:?}, Radiance's equal half is not added"
        );
        assert!(
            row(&on, "Protective_Beam", "absorb") > row(&off, "Protective_Beam", "absorb"),
            "{dataset:?}: Protective Beam's Radiance absorb is not added"
        );
        assert!(
            damage(&on, "Searing_Beam") > damage(&off, "Searing_Beam"),
            "{dataset:?}: Searing Beam's Radiance damage over time is missing"
        );
    }
}
