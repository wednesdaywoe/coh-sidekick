//! Spines' poison counts the foe's Poison stacks (`target.ownPowerNum?`), which no build can
//! know, so the player states the count with a slider. Unstated is a fresh target: zero.
//!
//! Before this the poison came through as an unresolved line with no number, and every Spines
//! hit's total left it out (Brute Impale 175 on Homecoming, 75 on the Brainstorm beta).

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::damage::DamageApplication;
use std::path::PathBuf;

const POISON: &str = "Temporary_Powers.Temporary_Powers.Poison";
const SET: &str = "brute/spines";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// A level-50 Brute holding Barb Swipe, against a foe carrying `stacks` Poison (`None` = unsaid).
fn build(dataset: DatasetId, db: &PowerDatabase, stacks: Option<u32>) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("brute".into());
    state.primary.id = Some(SET.into());
    assert!(
        db.resolve_power(SET, "Barb_Swipe").is_some(),
        "{dataset:?}: no Barb Swipe"
    );
    state.primary.powers = vec![SelectedPower::picked("Barb_Swipe", SET, 1)];
    if let Some(count) = stacks {
        state.combat.target_stacks.insert(POISON.into(), count);
    }
    coh_data::sync_granted_powers(&mut state, db);
    state
}

/// Barb Swipe's Toxic damage: the per-tick base of each component that always lands, and how
/// many Toxic rows were left unresolved.
fn toxic(dataset: DatasetId, db: &PowerDatabase, stacks: Option<u32>) -> (Vec<f64>, usize, f64) {
    let totals = coh_math::recalculate(&build(dataset, db, stacks), db);
    let swipe = totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Barb_Swipe")
        .expect("Barb Swipe projected");
    let landed = swipe
        .damage
        .components
        .iter()
        .filter(|c| c.damage_type == "Toxic")
        .filter(|c| matches!(c.application, DamageApplication::Always))
        .map(|c| c.base)
        .collect();
    let unresolved = swipe
        .damage
        .unresolved
        .iter()
        .filter(|row| row.damage_type == "Toxic")
        .count();
    (landed, unresolved, swipe.damage.base)
}

#[test]
fn brainstorm_spines_counts_poison_up_to_ten() {
    let db = load(DatasetId::Brainstorm);
    let state = build(DatasetId::Brainstorm, &db, None);
    let stacks = coh_data::target_stacks::build_target_stacks(&state, &db).expect("decodes");
    assert_eq!(stacks.len(), 1, "{stacks:?}");
    assert_eq!(stacks[0].path, POISON);
    assert_eq!(stacks[0].name, "Poison");
    assert_eq!(
        stacks[0].max_count,
        Some(10),
        "the Poison record's num_allowed"
    );
}

#[test]
fn unstated_poison_is_a_fresh_target() {
    let db = load(DatasetId::Brainstorm);
    let (unsaid, unresolved, hit) = toxic(DatasetId::Brainstorm, &db, None);
    assert_eq!(unresolved, 0, "the poison is still waiting on the target");
    assert_eq!(unsaid.len(), 1, "one poison component lands: {unsaid:?}");
    assert!(unsaid[0] > 0.0);
    assert_eq!(toxic(DatasetId::Brainstorm, &db, Some(0)), (unsaid, 0, hit));
}

#[test]
fn poison_grows_with_the_stated_stacks() {
    let db = load(DatasetId::Brainstorm);
    let (fresh, _, fresh_hit) = toxic(DatasetId::Brainstorm, &db, None);
    for count in [1, 4, 10] {
        let (ticks, unresolved, hit) = toxic(DatasetId::Brainstorm, &db, Some(count));
        assert_eq!(unresolved, 0);
        // The payload's own program: `stacks × .9 + 1`, times the standard result.
        let expected = fresh[0] * (f64::from(count) * 0.9 + 1.0);
        assert!(
            (ticks[0] - expected).abs() < 1e-6,
            "{count} stacks: tick {} vs {expected}",
            ticks[0]
        );
        assert!(
            hit > fresh_hit,
            "{count} stacks: the hit total did not grow"
        );
    }
}

#[test]
fn homecoming_spines_counts_no_stacks() {
    let db = load(DatasetId::Homecoming);
    let state = build(DatasetId::Homecoming, &db, None);
    let stacks = coh_data::target_stacks::build_target_stacks(&state, &db).expect("decodes");
    assert!(stacks.is_empty(), "{stacks:?}");
    let unsaid = toxic(DatasetId::Homecoming, &db, None);
    assert!(
        !unsaid.0.is_empty(),
        "Homecoming's inline poison left Barb Swipe"
    );
    assert_eq!(toxic(DatasetId::Homecoming, &db, Some(4)), unsaid);
}
