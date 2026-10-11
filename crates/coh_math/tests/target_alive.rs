//! A target a power is landing on is alive, which settles `kHitPoints% target> 0 >` and its
//! `Cur.kHitPoints` twin for that power's own projection.
//!
//! Before this the reader was Indeterminate, so DNA Siphon's whole hit (both its damage rows ask
//! it) and Glitz's target missile sat unresolved: listed, never valued. The totals context has no
//! one target and still leaves the reader open, which is why no build total moved.

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

/// The projected damage of one primary power: its landing total and the gates left unresolved.
fn damage(dataset: DatasetId, archetype: &str, set: &str, pick: &str) -> (f64, Vec<String>) {
    let db = load(dataset);
    assert!(
        db.resolve_power(set, pick).is_some(),
        "{dataset:?}: no {pick}"
    );
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some(archetype.into());
    state.primary.id = Some(set.into());
    state.primary.powers = vec![SelectedPower::picked(pick, set, 1)];
    coh_data::sync_granted_powers(&mut state, &db);
    let totals = coh_math::recalculate(&state, &db);
    let projected = totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == pick)
        .unwrap_or_else(|| panic!("{pick} projected"));
    let unresolved = projected
        .damage
        .unresolved
        .iter()
        .map(|row| row.gate.clone())
        .collect();
    (projected.damage.base, unresolved)
}

fn asks_alive(gate: &str) -> bool {
    gate.to_ascii_lowercase().contains("khitpoints")
}

#[test]
fn dna_siphon_lands_its_hit() {
    for dataset in [
        DatasetId::Homecoming,
        DatasetId::Rebirth,
        DatasetId::Thunderspy,
        DatasetId::Brainstorm,
    ] {
        let (base, unresolved) = damage(dataset, "tanker", "tanker/bio-armor", "DNA_Siphon");
        assert!(base > 0.0, "{dataset:?}: DNA Siphon deals nothing");
        assert!(
            !unresolved.iter().any(|gate| asks_alive(gate)),
            "{dataset:?}: still waiting on the target's hit points: {unresolved:?}"
        );
    }
}

#[test]
fn glitz_values_its_target_missile() {
    let (_, unresolved) = damage(
        DatasetId::Homecoming,
        "controller",
        "controller/pyrotechnic-control",
        "Glitz",
    );
    // The target missile asks only that the foe is alive; Containment's bonus also asks whether
    // it is mezzed, which no projection knows, so that pair stays open.
    let missile = "Cur.kHitPoints target> 0 > enttype target> critter eq &&";
    assert!(
        !unresolved.iter().any(|gate| gate == missile),
        "Glitz's target missile still waits: {unresolved:?}"
    );
    assert!(
        unresolved.iter().any(|gate| gate.contains("kHeld target>")),
        "Containment's bonus resolved without knowing whether the foe is mezzed: {unresolved:?}"
    );
}
