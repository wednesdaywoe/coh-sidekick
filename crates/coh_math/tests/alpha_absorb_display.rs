//! Protective Beam's absorb row read its enhancement off `heal`, so the Resilient Radial Alpha's
//! +33% Absorb (an Absorb-only boost, `alpha_silent/intangible_very_rare`) never showed, while
//! Vigor's Heal-only boost (`heal_plus_very_rare`, no Absorb attrib) wrongly did. A Healing IO
//! is one boost over both attribs, so it must still move the row.

use coh_data::{
    CharacterState, DatasetId, Enhancement, IncarnateSlot, PowerDatabase, SelectedPower,
};
use std::path::PathBuf;

const SET: &str = "defender/light-affinity";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Protective Beam's enhanced absorb for a level-50 Defender with this Alpha and slotting.
fn absorb(db: &PowerDatabase, alpha: Option<&str>, slots: Vec<Option<Enhancement>>) -> f64 {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = 50;
    state.archetype.id = Some("defender".into());
    state.primary.id = Some(SET.into());
    let mut power = SelectedPower::picked("Protective_Beam", SET, 1);
    power.slots = slots;
    state.primary.powers = vec![power];
    state.incarnates.alpha = alpha.map(|name| IncarnateSlot {
        power_name: name.into(),
        active: true,
    });
    coh_math::recalculate(&state, db)
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Protective_Beam")
        .expect("Protective Beam projected")
        .granted_magnitudes
        .iter()
        .find(|row| row.effect_key == "absorb")
        .expect("Protective Beam has an absorb row")
        .value
        .enhanced
}

#[test]
fn absorb_row_follows_absorb_boosts_only() {
    let db = load(DatasetId::Homecoming);
    let bare = absorb(&db, None, vec![]);

    let resilient = absorb(&db, Some("Resilient_Radial_Paragon"), vec![]);
    assert!(
        (resilient / bare - 1.33).abs() < 1e-6,
        "Resilient Radial: {resilient} vs {bare}"
    );

    let vigor = absorb(&db, Some("Vigor_Core_Paragon"), vec![]);
    assert_eq!(vigor, bare, "Vigor boosts Heal_Dmg only, never Absorb");

    let healing_io = absorb(
        &db,
        None,
        vec![Some(Enhancement::generic_io("Healing", None, 0))],
    );
    assert!(healing_io > bare, "a Healing IO enhances Absorb too");
}
