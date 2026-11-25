//! A buff applied twice at different lengths shows both. Inner Light grants +72% damage and
//! +18% to-hit for 10s, then +28.8% and +6.925% for 30s; the card showed only the 30s pair,
//! because the shorter instance rides the slot as `durationVariants` and nothing drew it.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use std::path::PathBuf;

const SET: &str = "peacebringer/luminous-blast";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Inner Light's rows for `key`, as (base, seconds), on a level-50 Peacebringer.
fn rows(dataset: DatasetId, key: &str) -> Vec<(f64, Option<f64>)> {
    let db = load(dataset);
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("peacebringer".into());
    state.primary.id = Some(SET.into());
    state.primary.powers = vec![SelectedPower::picked("Build_Up", SET, 12)];
    coh_math::recalculate(&state, &db)
        .power_projection
        .into_iter()
        .find(|p| p.power_internal_name == "Build_Up")
        .expect("Inner Light projected")
        .granted_magnitudes
        .into_iter()
        .filter(|row| row.effect_key == key)
        .map(|row| (row.value.base, row.duration))
        .collect()
}

fn round(rows: Vec<(f64, Option<f64>)>) -> Vec<(f64, Option<f64>)> {
    rows.into_iter()
        .map(|(base, seconds)| ((base * 1000.0).round() / 1000.0, seconds))
        .collect()
}

#[test]
fn inner_light_shows_its_burst_and_its_tail() {
    // dataset-absent: rebirth, thunderspy — both carry the same pair; Homecoming and its beta
    // are the forks the report came from.
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        assert_eq!(
            round(rows(dataset, "damageBuff")),
            vec![(28.8, Some(30.0)), (72.0, Some(10.0))],
            "{dataset:?} damage"
        );
        assert_eq!(
            round(rows(dataset, "tohitBuff")),
            vec![(6.93, Some(30.0)), (18.0, Some(10.0))],
            "{dataset:?} to-hit"
        );
    }
}
