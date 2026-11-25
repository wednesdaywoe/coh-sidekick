//! Same-internalName powers in a Dominator's secondary and its matching epic rep
//! address separately — the pick the rail marks is the pick that set's own.
//!
//! Reported by a user 2026-11-24 on a fresh Fire/Fire/Fire Dominator: "Selecting Fire
//! Blast from Fiery Assault is also selecting Rain of Fire from Fire Mastery. Selecting
//! Consume from Fiery Assault is also selecting Melt Armor from Fire Mastery. Doing the
//! inverse is also true", with the Ice/Ice/Ice pair (Ice Blast/Sleet, Frost
//! Breath/Hoarfrost) failing the same way. The user's hypothesis — the pairs "sharing a
//! database id" — is nearly right: the game renamed the epic masteries' powers and kept
//! the originals' internal names, so Homecoming's bundle really carries `Fire_Blast`
//! twice (displayed "Fire Blast" in Fiery Assault, "Rain of Fire" in Fire Mastery),
//! `Consume` twice ("Consume"/"Melt Armor"), `Ice_Blast` ("Ice Blast"/"Sleet") and
//! `Frost_Breath` ("Frost Breath"/"Hoarfrost"). The defect was the Available rail's
//! picked-row marking, which consulted a build-wide list keyed by internal name alone:
//! holding one twin marked the OTHER twin's row taken, and since a marked row toggles, a
//! click on the twin gave nothing back and took nothing — a selection the two powers
//! shared. The marking moved to `CharacterState::selected_power`, which addresses a pick
//! by set id + internal name; this pins that address for exactly the pairs the user
//! named, against the real bundle, so a regression re-marking by name fails here first.

use coh_data::{CharacterState, DatasetId, Power, PowerDatabase, SelectedPower};
use std::path::PathBuf;

fn homecoming() -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(DatasetId::Homecoming.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
}

/// The four pairs the user reported: secondary set id, epic pool id, shared internal
/// name, and the display name each twin wears.
const COLLIDING_PAIRS: [(&str, &str, &str, (&str, &str)); 4] = [
    (
        "dominator/fiery-assault",
        "fire_mastery_dominator",
        "Fire_Blast",
        ("Fire Blast", "Rain of Fire"),
    ),
    (
        "dominator/fiery-assault",
        "fire_mastery_dominator",
        "Consume",
        ("Consume", "Melt Armor"),
    ),
    (
        "dominator/icy-assault",
        "ice_mastery_dominator",
        "Ice_Blast",
        ("Ice Blast", "Sleet"),
    ),
    (
        "dominator/icy-assault",
        "ice_mastery_dominator",
        "Frost_Breath",
        ("Frost Breath", "Hoarfrost"),
    ),
];

fn epic_power<'a>(db: &'a PowerDatabase, set_id: &str, ident: &str) -> Option<&'a Power> {
    db.epic_powers
        .iter()
        .find(|entry| entry.set_id == set_id && entry.power.ident() == ident)
        .map(|entry| &entry.power)
}

/// The dataset really does republish the internal names, so the address (not the name)
/// is what separates the twins — if a fork ever renames one twin, this test says so and
/// the pairs above are edited rather than trusted.
#[test]
fn dominator_secondaries_and_their_epic_reps_share_internal_names() {
    let db = homecoming();
    for (secondary, epic, ident, (sec_display, epic_display)) in COLLIDING_PAIRS {
        let secondary_power = db
            .find_powerset(secondary)
            .unwrap_or_else(|| panic!("{secondary} is in the bundle"))
            .powers
            .iter()
            .find(|power| power.ident() == ident)
            .unwrap_or_else(|| panic!("{secondary} offers {ident}"));
        let epic_power =
            epic_power(&db, epic, ident).unwrap_or_else(|| panic!("{epic} republishes {ident}"));
        assert_eq!(secondary_power.name, sec_display, "{secondary}/{ident}");
        assert_eq!(epic_power.name, epic_display, "{epic}/{ident}");
        assert_ne!(
            secondary_power.name, epic_power.name,
            "the twins must differ by display name — the collision is the point"
        );
    }
}

/// A dominator holding the build halves of one pair: the secondary picks into the
/// secondary bucket, the epic picks into the epic bucket, both addressed by set + name.
fn dominator(secondary: &str, epic: &str) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.secondary.id = Some(secondary.to_string());
    state.epic_pool = Some(coh_data::PoolSelection {
        id: epic.to_string(),
        name: epic.to_string(),
        powers: Vec::new(),
    });
    state
}

#[test]
fn holding_one_twin_marks_only_its_own_set_row() {
    for (secondary, epic, ident, _) in COLLIDING_PAIRS {
        let mut state = dominator(secondary, epic);
        state
            .secondary
            .powers
            .push(SelectedPower::picked(ident, secondary, 10));
        let held = state
            .selected_power(secondary, ident)
            .unwrap_or_else(|| panic!("{secondary}/{ident} resolves against its own set"));
        assert_eq!(held.level, 10);
        assert!(
            state.selected_power(epic, ident).is_none(),
            "holding {secondary}/{ident} must not resolve an {epic} address — \
             that is the shared marking the user reported",
        );

        // The inverse seat, on a fresh build: the epic twin alone must not mark the
        // secondary row either.
        let mut state = dominator(secondary, epic);
        state
            .epic_pool
            .as_mut()
            .unwrap()
            .powers
            .push(SelectedPower::picked(ident, epic, 35));
        assert_eq!(state.selected_power(epic, ident).map(|p| p.level), Some(35));
        assert!(
            state.selected_power(secondary, ident).is_none(),
            "holding {epic}/{ident} must not resolve a {secondary} address",
        );
    }
}

#[test]
fn both_twins_can_be_held_at_once_and_edited_apart() {
    for (secondary, epic, ident, _) in COLLIDING_PAIRS {
        let mut state = dominator(secondary, epic);
        state
            .secondary
            .powers
            .push(SelectedPower::picked(ident, secondary, 10));
        state
            .epic_pool
            .as_mut()
            .unwrap()
            .powers
            .push(SelectedPower::picked(ident, epic, 35));

        // Each address reads its own level, not the other's.
        assert_eq!(
            state.selected_power(secondary, ident).map(|p| p.level),
            Some(10)
        );
        assert_eq!(state.selected_power(epic, ident).map(|p| p.level), Some(35));

        // And the one write handle every per-power edit routes through reaches exactly
        // one twin.
        state.selected_power_mut(epic, ident).unwrap().is_active = true;
        assert!(state.selected_power(epic, ident).unwrap().is_active);
        assert!(
            !state.selected_power(secondary, ident).unwrap().is_active,
            "toggling the {epic} twin flipped the {secondary} twin too",
        );
    }
}
