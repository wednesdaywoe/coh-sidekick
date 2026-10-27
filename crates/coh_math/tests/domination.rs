//! A Dominator's Domination switch moves numbers. With it on, each control power's card shows the
//! Domination copy of its mez as a row of its own (half again as long, beside the base one), and
//! Domination's own buff counts in the totals: +25% ToHit on Homecoming, none on Rebirth, mez
//! protection on both. Before, the copy was dropped as a collision and the power never counted.

use coh_data::{CharacterState, DatasetId, InherentCategory, PowerDatabase, SelectedPower};
use std::path::PathBuf;

const SET: &str = "dominator/mind-control";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn dominator(dataset: DatasetId, domination: bool) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("dominator".into());
    state.primary.id = Some(SET.into());
    state.primary.powers = vec![SelectedPower::picked("Mesmerize", SET, 1)];
    let mut inherent = SelectedPower::picked("Domination", coh_data::INHERENT_SET, 1);
    inherent.inherent_category = Some(InherentCategory::Archetype);
    state.inherents = vec![inherent];
    state
        .combat
        .global_conditionals
        .insert("domination".into(), domination);
    state
}

/// Mesmerize's Sleep rows as (label, seconds).
fn sleep_rows(db: &PowerDatabase, state: &CharacterState) -> Vec<(String, f64)> {
    coh_math::recalculate(state, db)
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Mesmerize")
        .expect("Mesmerize projected")
        .granted_magnitudes
        .iter()
        .filter(|row| row.effect_key == "sleep")
        .map(|row| (row.label.clone(), row.value.base))
        .collect()
}

#[test]
fn domination_adds_a_longer_sleep_row() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let db = load(dataset);
        let off = sleep_rows(&db, &dominator(dataset, false));
        assert_eq!(
            off.len(),
            1,
            "{dataset:?}: one Sleep row without Domination: {off:?}"
        );

        let on = sleep_rows(&db, &dominator(dataset, true));
        assert_eq!(on.len(), 2, "{dataset:?}: Domination adds a row: {on:?}");
        assert_eq!(on[0], off[0], "{dataset:?}: the base row is unchanged");
        assert_eq!(on[1].0, "Sleep (Domination Active)");
        assert!(
            (on[1].1 / off[0].1 - 1.5).abs() < 1e-6,
            "{dataset:?}: Domination's Sleep lasts half again as long: {on:?}"
        );
    }
}

#[test]
fn domination_buff_counts_while_on() {
    for (dataset, to_hit) in [(DatasetId::Homecoming, 25.0), (DatasetId::Rebirth, 0.0)] {
        let db = load(dataset);
        let off = coh_math::recalculate(&dominator(dataset, false), &db).bonuses;
        let on = coh_math::recalculate(&dominator(dataset, true), &db).bonuses;
        assert_eq!(off.to_hit, 0.0, "{dataset:?}");
        assert!(
            (on.to_hit - to_hit).abs() < 1e-6,
            "{dataset:?}: ToHit {}",
            on.to_hit
        );
        assert_eq!(off.protection_hold, 0.0, "{dataset:?}");
        assert!(
            on.protection_hold > 0.0,
            "{dataset:?}: Domination protects from holds"
        );
    }
}

/// Domination's own ON pill is the same state as the Combat panel's switch: either one turns
/// both the card row and the buff on, and the Combat switch moves the pill with it.
#[test]
fn the_pill_and_the_combat_switch_are_one_state() {
    let dataset = DatasetId::Homecoming;
    let db = load(dataset);

    let mut by_pill = dominator(dataset, false);
    by_pill.combat.global_conditionals.clear();
    by_pill.inherents[0].is_active = true;
    assert_eq!(
        sleep_rows(&db, &by_pill).len(),
        2,
        "the pill shows the Domination row"
    );
    let to_hit = coh_math::recalculate(&by_pill, &db).bonuses.to_hit;
    assert!(
        (to_hit - 25.0).abs() < 1e-6,
        "the pill counts the buff: {to_hit}"
    );

    let mut by_switch = dominator(dataset, false);
    let mechanics = coh_data::global_mechanics(&by_switch, &db);
    coh_data::set_global_mechanic(&mut by_switch, &mechanics, "domination", true);
    assert!(
        by_switch.inherents[0].is_active,
        "the Combat switch moves the pill"
    );
    coh_data::mirror_inherent_switch(&mut by_switch, "Domination", false);
    assert_eq!(
        by_switch.combat.global_conditionals.get("domination"),
        Some(&false),
        "the pill moves the Combat switch"
    );
}
