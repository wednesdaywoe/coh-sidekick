//! A stance option imported from the game does not spend a power pick.
//!
//! Reported by a user 2026-11-04: pulling a Tanker Bio Armor / Spines build out of
//! Homecoming's Vault into Sidekick showed
//! `1 power(s) sit at a level this dataset grants no pick at: Physical_Perfection`
//! on a level-50 build, and unselecting and reselecting the L44 pick changed nothing.
//!
//! The build arrived carrying three powers it never bought. `Efficient_Adaptation`,
//! `Defensive_Adaptation` and `Offensive_Adaptation` are the forms Bio Armor's Adaptation
//! hands out; the game issues them, so the client lists all three as owned at level 0, and
//! both game formats carry them into the build as ordinary picks. The grant reconcile
//! deliberately does NOT materialize a stance option — the parent pick's `active_sub_power`
//! is their surface, and materializing them would double-count the live form — so nothing
//! claimed the three entries, and each one spent a power pick. Twenty-five picks into
//! Homecoming's twenty-four cells: every pick from L1 up shifted one cell per phantom, and
//! the topmost power fell off the end of the by-level grid.
//!
//! Four legs, because the defect has four faces:
//!
//! * the reported file opens clean — the invariant the by-level grid lays itself out from,
//!   graded against the schedule rather than against a copy of the dealing code;
//! * the power is re-homed, not lost — the stance control still offers all three forms;
//! * a save written from the healed build no longer carries them, so the file stops
//!   re-infecting itself;
//! * an option whose parent the build does NOT hold survives, since dropping that one would
//!   shrink a build on import.

use coh_data::{skif, CharacterState, DatasetId, LevelingSchedule, PowerDatabase, SelectedPower};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn homecoming() -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(DatasetId::Homecoming.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load homecoming: {e}"))
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/qa")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// The two reconciles `build_io::adopt_as` runs on a load, in its order — the identity sync
/// first, the grant sync second. Inlined because the app crate has no lib target; the ORDER
/// is half of what is under test, as in `grant_survives_a_read.rs`.
fn load_reconcile(state: &mut CharacterState, db: &PowerDatabase) {
    if state.archetype.id.is_some() {
        if let Ok(Some(grants)) = db.inherent_grants() {
            let auto = db
                .leveling_schedule
                .as_ref()
                .map(|schedule| schedule.auto_granted_slot_levels.clone())
                .unwrap_or_default();
            state.inherents =
                coh_data::granted_inherents(&grants, None, &auto, state.level, &state.inherents);
        }
    }
    coh_data::sync_granted_powers(state, db);
}

/// The reported build, as the app holds it a moment after opening the file.
fn opened(db: &PowerDatabase) -> CharacterState {
    let mut build = skif::decode(&fixture("vault-bio-armor-probe.skif"), db)
        .expect("the reported file is a v5 .skif")
        .build;
    load_reconcile(&mut build, db);
    build
}

/// Every level the build has spent a pick at, tallied.
fn spent(build: &CharacterState) -> BTreeMap<u8, usize> {
    let mut tally = BTreeMap::new();
    for power in build.picked_powers() {
        *tally.entry(power.level).or_default() += 1;
    }
    tally
}

/// What the by-level grid asks of a build, in the schedule's own words: no level may hold
/// more picks than it grants, and no pick may sit at a level that grants none. That is the
/// exact condition `deal_picks` needs to place every power in a cell of its own — a build
/// that breaks it has a power with nowhere to go, which is the error the user saw.
fn overdrawn(schedule: &LevelingSchedule, build: &CharacterState) -> Vec<String> {
    spent(build)
        .into_iter()
        .filter_map(|(level, used)| {
            let granted = schedule.picks_granted_at_level(level) as usize;
            (used > granted).then(|| format!("L{level} holds {used} pick(s), grants {granted}"))
        })
        .collect()
}

#[test]
fn the_reported_build_fits_the_schedule_it_was_imported_against() {
    let db = homecoming();
    let build = opened(&db);
    let schedule = db
        .leveling_schedule
        .as_ref()
        .expect("homecoming ships a leveling schedule");
    assert_eq!(
        overdrawn(schedule, &build),
        Vec::<String>::new(),
        "the imported build spends more picks than the schedule grants, so the by-level \
         grid has a power with nowhere to go"
    );
    // The three phantoms are what did it. Named, so a fix that drops something ELSE is
    // caught by the stance leg below rather than passing on the count alone.
    let names: Vec<&str> = build
        .picked_powers()
        .map(|power| power.internal_name.as_str())
        .collect();
    for form in [
        "Efficient_Adaptation",
        "Defensive_Adaptation",
        "Offensive_Adaptation",
    ] {
        assert!(
            !names.contains(&form),
            "{form} is still counted as a bought pick"
        );
    }
    // And the build is otherwise whole: 22 bought picks, the topmost one intact.
    assert_eq!(
        names.len(),
        22,
        "the build lost or gained a pick: {names:?}"
    );
    assert!(names.contains(&"Physical_Perfection"));
}

/// Dropping the option must not lose the power. Bio Armor's Adaptation is the internal
/// name `Evolution` (the L14 pick is `Adaptation`, shown as "Evolving Armor"), and its
/// three forms are the strip under that card.
#[test]
fn the_forms_are_still_offered_by_their_parent() {
    let db = homecoming();
    let build = opened(&db);
    let group = coh_data::stance_group_for(&build, &db, "tanker/bio-armor", "Evolution")
        .expect("a build holding Adaptation holds its stance selector");
    let options: Vec<&str> = group
        .options
        .iter()
        .map(|o| o.internal_name.as_str())
        .collect();
    assert_eq!(
        options,
        [
            "Efficient_Adaptation",
            "Defensive_Adaptation",
            "Offensive_Adaptation"
        ]
    );
}

/// A save written from the healed build carries no options, so opening it again cannot
/// re-grow the phantoms. The writer records every selection in the bucket, so this fails
/// the moment the reconcile stops dropping them.
#[test]
fn a_save_from_the_healed_build_does_not_carry_them_again() {
    let db = homecoming();
    let build = opened(&db);
    let text = skif::encode(&build, &db).expect("the healed build saves");
    let mut again = skif::decode(&text, &db).expect("the save reads back").build;
    load_reconcile(&mut again, &db);
    let schedule = db.leveling_schedule.as_ref().unwrap();
    assert_eq!(overdrawn(schedule, &again), Vec::<String>::new());
    let held: Vec<&str> = again
        .primary
        .powers
        .iter()
        .map(|power| power.internal_name.as_str())
        .collect();
    assert!(!held.contains(&"Efficient_Adaptation"), "{held:?}");
    assert!(
        held.contains(&"Evolution"),
        "the parent went with it: {held:?}"
    );
}

/// The guard against over-reaching: an option whose PARENT the build does not hold is left
/// where it is. The client only ever lists an option beside its parent, so this is not the
/// reported case — it is the case where dropping the entry would silently shrink a build,
/// which `skif.rs` rule 8 refuses outright.
#[test]
fn an_option_without_its_parent_is_left_alone() {
    let db = homecoming();
    let mut build = opened(&db);
    build.primary.powers.retain(|power| {
        power.internal_name != "Evolution" && !power.internal_name.ends_with("_Adaptation")
    });
    build.primary.powers.push(option("Efficient_Adaptation"));
    coh_data::sync_granted_powers(&mut build, &db);
    let held: Vec<&str> = build
        .primary
        .powers
        .iter()
        .map(|power| power.internal_name.as_str())
        .collect();
    assert!(
        held.contains(&"Efficient_Adaptation"),
        "an option nothing owns was dropped: {held:?}"
    );
}

fn option(name: &str) -> SelectedPower {
    let mut power = SelectedPower::picked(name, "tanker/bio-armor", 0);
    power.slots = vec![];
    power
}
