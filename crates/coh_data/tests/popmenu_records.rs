//! The popmenu writes the game's own name for an enhancement, on every fork.
//!
//! A `.mnu` is executed by the game, not read back by us, and `boost` answers a record it does
//! not know with a console line among seventy others. So a wrong name is an enhancement that is
//! simply not in the tray, with nothing saying why — which is how the beta spelled Fly
//! `Crafted_Flight` and Slow `Crafted_Slow` for eleven months (POPMENU-1).
//!
//! The oracle is `exported_powers/<fork>/boosts/`, read off disk — the export's own roster of
//! record names, not the boost index the exporter resolves through. An index built wrong would
//! agree with an exporter reading it; the directory is the third party. Names compare
//! case-insensitively because the game's lookups do.

use coh_data::popmenu::{self, WarningKind};
use coh_data::{CharacterState, DatasetId, Enhancement, IoSlotting, PowerDatabase, SelectedPower};
use std::collections::HashSet;
use std::path::PathBuf;

const DATASETS: [DatasetId; 4] = [
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Thunderspy,
    DatasetId::Brainstorm,
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = root()
        .join("contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Every record name the export ships, lower-cased: one directory per boost record.
fn exported_records(dataset: DatasetId) -> HashSet<String> {
    let mut dir = root().join("exported_powers");
    if dataset != DatasetId::Homecoming {
        dir = dir.join(dataset.as_str());
    }
    std::fs::read_dir(dir.join("boosts"))
        .unwrap_or_else(|e| panic!("{dataset:?} boosts: {e}"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_lowercase())
        .collect()
}

fn power(name: &str, slots: Vec<Enhancement>) -> SelectedPower {
    SelectedPower {
        internal_name: name.to_string(),
        powerset: "test/test".to_string(),
        level: 1,
        slots: slots.into_iter().map(Some).collect(),
        is_active: false,
        active_sub_power: None,
        inherent_slot_count: 0,
        is_locked: false,
        inherent_category: None,
        targets_hit: None,
        target_distance: None,
    }
}

/// A build holding exactly these slots in one primary power. A skeleton rather than a real
/// powerset: the exporter reads slots and names, and the four forks' rosters differ.
fn build_with(dataset: DatasetId, slots: Vec<Enhancement>) -> CharacterState {
    let mut build = CharacterState::empty(dataset);
    build.primary.powers.push(power("Test Power", slots));
    build
}

/// The records a generated menu grants, in order.
fn granted(build: &CharacterState, db: &PowerDatabase) -> Vec<String> {
    let index = db.boost_index.as_ref().expect("boost index");
    let menu = popmenu::generate(build, db, index, "Test", "now");
    menu.content
        .split("$$")
        .flat_map(|chunk| chunk.split('"'))
        .filter_map(|part| part.strip_prefix("boost "))
        .map(|command| {
            let words: Vec<&str> = command.split(' ').collect();
            // The command names the set and the power, and for a boost record they are the
            // same name. A pair that disagreed would be a malformed command.
            assert_eq!(words.len(), 3, "{command}");
            assert_eq!(words[0], words[1], "{command}");
            words[0].to_string()
        })
        .collect()
}

fn granted_one(dataset: DatasetId, db: &PowerDatabase, enh: Enhancement) -> Option<String> {
    granted(&build_with(dataset, vec![enh]), db)
        .into_iter()
        .next()
}

#[test]
fn every_generic_io_names_a_shipped_record() {
    for dataset in DATASETS {
        let db = load(dataset);
        let shipped = exported_records(dataset);
        let catalog = db.enhancements.as_ref().expect("catalog");
        let unknown: Vec<String> = catalog
            .common_io_types
            .iter()
            .filter_map(|stat| {
                let uid = granted_one(dataset, &db, Enhancement::generic_io(stat, None, 0));
                match &uid {
                    Some(uid) if shipped.contains(&uid.to_lowercase()) => None,
                    _ => Some(format!("{stat} -> {uid:?}")),
                }
            })
            .collect();
        assert!(unknown.is_empty(), "{dataset:?}: {unknown:?}");
    }
}

#[test]
fn spells_the_two_the_beta_table_got_wrong() {
    for dataset in DATASETS {
        let db = load(dataset);
        let fly = granted_one(dataset, &db, Enhancement::generic_io("Fly", None, 0));
        let slow = granted_one(dataset, &db, Enhancement::generic_io("Slow", None, 0));
        assert_eq!(fly.as_deref(), Some("Crafted_Fly"), "{dataset:?}");
        assert_eq!(slow.as_deref(), Some("Crafted_Snare"), "{dataset:?}");
    }
}

#[test]
fn every_set_piece_names_a_shipped_record_or_says_why() {
    for dataset in DATASETS {
        let db = load(dataset);
        let shipped = exported_records(dataset);
        let index = db.boost_index.as_ref().expect("boost index");
        let sets = db.io_sets.as_ref().expect("io sets");
        let mut unknown = Vec::new();
        let mut named = 0;
        for (set_id, set) in &sets.sets {
            for (i, piece) in set.pieces.iter().enumerate() {
                for attuned in [false, true] {
                    let enh = Enhancement::io_set(
                        set_id,
                        &set.name,
                        i,
                        piece,
                        set.min_level,
                        set.max_level,
                        set.attuned_only,
                        IoSlotting {
                            attuned,
                            io_level: 50,
                            boost: 0,
                        },
                    );
                    let build = build_with(dataset, vec![enh]);
                    let menu = popmenu::generate(&build, &db, index, "Test", "now");
                    match granted(&build, &db).first() {
                        Some(uid) if shipped.contains(&uid.to_lowercase()) => named += 1,
                        Some(uid) => unknown.push(format!("{set_id} {} -> {uid}", piece.num)),
                        // A piece with no record is allowed — a craftable set with no attuned
                        // twin is real — but only when the exporter SAYS so.
                        None => assert_eq!(menu.warnings.len(), 1, "{set_id} {} silent", piece.num),
                    }
                }
            }
        }
        assert!(unknown.is_empty(), "{dataset:?}: {unknown:?}");
        assert!(named > 1000, "{dataset:?} named only {named} set pieces");
    }
}

#[test]
fn every_special_names_a_shipped_record() {
    for dataset in DATASETS {
        let db = load(dataset);
        let shipped = exported_records(dataset);
        let catalog = db.enhancements.as_ref().expect("catalog");
        let mut unknown = Vec::new();
        let mut count = 0;
        for (_, category, defs) in catalog.special_families() {
            for (id, def) in defs {
                count += 1;
                let uid = granted_one(dataset, &db, Enhancement::special(id, def, category, 0));
                match &uid {
                    Some(uid) if shipped.contains(&uid.to_lowercase()) => {}
                    _ => unknown.push(format!("{category}-{id} -> {uid:?}")),
                }
            }
        }
        assert!(unknown.is_empty(), "{dataset:?}: {unknown:?}");
        assert!(count > 0, "{dataset:?} carries no specials");
    }
}

#[test]
fn every_slot_is_a_command_or_a_warning() {
    // The conservation law this exporter exists for: a slot that is neither granted nor
    // reported is the silent drop, whatever the reason for it.
    let dataset = DatasetId::Homecoming;
    let db = load(dataset);
    let index = db.boost_index.as_ref().expect("boost index");
    let slots = vec![
        Enhancement::generic_io("Fly", None, 0),
        Enhancement::generic_io("Slow", None, 0),
        Enhancement::origin("Damage", "SO", None, 0),
    ];
    let count = slots.len();
    let build = build_with(dataset, slots);
    let menu = popmenu::generate(&build, &db, index, "Test", "now");
    assert_eq!(granted(&build, &db).len() + menu.warnings.len(), count);
    // The origin is the reported one, as a policy rather than a missing record.
    assert_eq!(menu.warnings.len(), 1);
    assert_eq!(menu.warnings[0].kind, WarningKind::NotGrantable);
    assert_eq!(menu.warnings[0].power, "Test Power");
    assert_eq!(menu.warnings[0].slot, 3);
}

#[test]
fn long_builds_split_across_options() {
    let dataset = DatasetId::Homecoming;
    let db = load(dataset);
    let index = db.boost_index.as_ref().expect("boost index");
    let slots = (0..100)
        .map(|_| Enhancement::generic_io("Recharge", None, 0))
        .collect();
    let build = build_with(dataset, slots);
    let menu = popmenu::generate(&build, &db, index, "Test", "now");
    assert!(menu
        .content
        .contains("Option \"Give enhancements (part 1)\""));
    assert!(menu
        .content
        .contains("Option \"Give enhancements (part 2)\""));
    assert!(!menu.content.contains("(part 3)"));
    assert_eq!(granted(&build, &db).len(), 100);
}

#[test]
fn the_reported_swift_fly_io() {
    // The Wailing Dead, 2026-09-21: the only command the game rejected was the Fly IO in
    // Swift, an inherent. Boosted, which the command cannot carry, so it arrives at 50.
    let dataset = DatasetId::Brainstorm;
    let db = load(dataset);
    let index = db.boost_index.as_ref().expect("boost index");
    let mut build = CharacterState::empty(dataset);
    build.inherents.push(power(
        "Swift",
        vec![Enhancement::generic_io("Fly", coh_data::Level::new(50), 5)],
    ));
    let menu = popmenu::generate(&build, &db, index, "Test", "now");
    assert!(menu.warnings.is_empty(), "{:?}", menu.warnings);
    assert!(menu.content.contains("boost Crafted_Fly Crafted_Fly 50"));
}
