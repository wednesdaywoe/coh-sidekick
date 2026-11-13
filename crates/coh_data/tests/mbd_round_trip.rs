//! Each build Mids wrote, read in the way the app reads it and written back out, matches the
//! file Mids wrote. This is the guard the beta's round trip was: it found MBDEXPORT-15, -16, -18.
//!
//! Every difference that's allowed is one named rule below with its reason; anything else fails.

use coh_data::mbd::{self, MbdFile, MbdPowerEntry};
use coh_data::mbd_export::write_mbd;
use coh_data::mbd_import::to_skif_build;
use coh_data::skif::{hydrate, SkifFile, VERSION};
use coh_data::{
    granted_inherents, sync_granted_powers, CharacterState, DatasetId, InherentCategory,
    InherentGrant, Level, PowerDatabase,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const DEFAULT_IO_LEVEL: Level = Level::constant(50);

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn fixtures(dataset: DatasetId) -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mids")
        .join(dataset.as_str());
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "mbd"))
        .collect();
    paths.sort();
    paths
}

/// The app's own load: convert, hydrate, then the two grant syncs `build_io::adopt_as` runs.
/// Also returns what the importer itself declined, by the name the file gave it.
fn as_the_app_opens(
    file: &MbdFile,
    database: &PowerDatabase,
    dataset: DatasetId,
) -> (CharacterState, BTreeSet<String>) {
    let converted = to_skif_build(file, database, dataset);
    let declined = converted
        .unresolved
        .iter()
        .map(|note| note.context.clone())
        .collect();
    let skif = SkifFile {
        version: VERSION,
        authored_against: None,
        meta: None,
        build: converted.build,
    };
    let mut state = hydrate(skif, database).expect("the build hydrates").build;
    let grants = database
        .inherent_grants()
        .unwrap()
        .expect("an inherent list");
    let auto = database
        .leveling_schedule
        .as_ref()
        .map(|schedule| schedule.auto_granted_slot_levels.clone())
        .unwrap_or_default();
    let archetype = archetype_inherent(&state, database);
    state.inherents = granted_inherents(
        &grants,
        archetype.as_ref(),
        &auto,
        state.level,
        &state.inherents,
    );
    sync_granted_powers(&mut state, database);
    (state, declined)
}

/// The app's `inherents::archetype_inherent`, which lives in the app crate.
fn archetype_inherent(state: &CharacterState, db: &PowerDatabase) -> Option<InherentGrant> {
    let declared = db
        .archetypes()
        .ok()?
        .get(state.archetype.id.as_deref()?)?
        .inherent
        .name
        .clone();
    let power = db.archetype_inherent(&declared)?;
    Some(InherentGrant {
        internal_name: power.ident().to_string(),
        name: power.name.clone(),
        category: InherentCategory::Archetype,
        max_slots: power.max_slots().unwrap_or(0),
        full_name: None,
        granted_at_level: None,
    })
}

/// One field that differs between Mids' row and ours.
#[derive(Debug)]
struct Difference {
    power: String,
    what: String,
    mids: String,
    ours: String,
}

fn entry_differences(original: &MbdPowerEntry, written: &MbdPowerEntry) -> Vec<Difference> {
    let mut found = Vec::new();
    let mut differ = |what: String, mids: String, ours: String| {
        if mids != ours {
            found.push(Difference {
                power: original.power_name.clone(),
                what,
                mids,
                ours,
            });
        }
    };
    differ(
        "level".into(),
        original.level.to_string(),
        written.level.to_string(),
    );
    differ(
        "StatInclude".into(),
        original.stat_include.to_string(),
        written.stat_include.to_string(),
    );
    differ(
        "VariableValue".into(),
        original.variable_value.to_string(),
        written.variable_value.to_string(),
    );
    differ(
        "slots".into(),
        original.slot_entries.len().to_string(),
        written.slot_entries.len().to_string(),
    );
    for (index, (a, b)) in original
        .slot_entries
        .iter()
        .zip(&written.slot_entries)
        .enumerate()
    {
        let slot = index + 1;
        differ(
            format!("slot {slot} level"),
            a.level.to_string(),
            b.level.to_string(),
        );
        differ(
            format!("slot {slot} IsInherent"),
            a.is_inherent.to_string(),
            b.is_inherent.to_string(),
        );
        let piece = |e: &Option<coh_data::mbd::MbdEnhancement>| {
            e.as_ref().map_or(String::new(), |e| {
                format!("{} {} {}", e.uid, e.grade, e.relative_level)
            })
        };
        differ(
            format!("slot {slot} piece"),
            piece(&a.enhancement),
            piece(&b.enhancement),
        );
    }
    found
}

fn differences(original: &MbdFile, written: &MbdFile) -> Vec<Difference> {
    let mut found = Vec::new();
    let file_level = |what: &str, mids: String, ours: String| Difference {
        power: String::new(),
        what: what.to_string(),
        mids,
        ours,
    };
    let last = |file: &MbdFile| usize::try_from(file.last_power).unwrap_or(0);
    for (index, original_entry) in original.power_entries.iter().enumerate() {
        if original_entry.power_name.is_empty() {
            continue;
        }
        let Some((at, ours)) = written
            .power_entries
            .iter()
            .enumerate()
            .find(|(_, e)| e.power_name == original_entry.power_name)
        else {
            found.push(Difference {
                power: original_entry.power_name.clone(),
                what: "row".into(),
                mids: "present".into(),
                ours: "missing".into(),
            });
            continue;
        };
        // Mids reads the picks by position: row i is the i-th level-up pick.
        if index < last(original) && at != index {
            found.push(Difference {
                power: original_entry.power_name.clone(),
                what: "pick row".into(),
                mids: index.to_string(),
                ours: at.to_string(),
            });
        }
        found.extend(entry_differences(original_entry, ours));
    }
    found.push(file_level(
        "LastPower",
        original.last_power.to_string(),
        written.last_power.to_string(),
    ));
    if original.power_sets != written.power_sets {
        found.push(file_level(
            "PowerSets",
            format!("{:?}", original.power_sets),
            format!("{:?}", written.power_sets),
        ));
    }
    for (what, mids, ours) in [
        ("Class", &original.class, &written.class),
        ("Origin", &original.origin, &written.origin),
        (
            "Database",
            &original.built_with.database,
            &written.built_with.database,
        ),
    ] {
        found.push(file_level(what, mids.clone(), ours.clone()));
    }
    found.retain(|d| d.mids != d.ours);
    found
}

/// Why a difference is allowed, or `None` where it isn't.
fn allowed(
    difference: &Difference,
    original: &MbdFile,
    written: &MbdFile,
    declined: &BTreeSet<String>,
    archetype_inherent: Option<&str>,
) -> Option<&'static str> {
    let row = original
        .power_entries
        .iter()
        .position(|e| e.power_name == difference.power);
    let entry = row.map(|index| &original.power_entries[index]);
    let granted =
        row.is_some_and(|index| index >= usize::try_from(original.last_power).unwrap_or(0));
    let holds_nothing =
        entry.is_some_and(|e| e.enhancement_count() == 0 && e.slot_entries.len() <= 1);
    let group = |prefix: &str| difference.power.starts_with(prefix);

    match difference.what.as_str() {
        "row" if declined.contains(&difference.power) => {
            Some("the importer declines it and reports why, so the writer never sees it")
        }
        // Accolades, incarnates and the archetype inherent are carried, so not this rule.
        "row"
            if granted
                && holds_nothing
                && !group("Temporary_Powers.")
                && !group("Incarnate.")
                && Some(difference.power.as_str()) != archetype_inherent =>
        {
            Some("a granted row holding nothing; the importer drops it and Mids re-creates it on load")
        }
        "LastPower"
            if usize::try_from(original.last_power - 1)
                .ok()
                .and_then(|index| original.power_entries.get(index))
                .is_some_and(|e| e.power_name.starts_with("Inherent.")) =>
        {
            Some("Mids counted its own inherent into the pick rows; the picks are the same")
        }
        "VariableValue" => Some("sliders aren't read from a .mbd or kept in a .skif yet (unfiled-115)"),
        "StatInclude" if granted && entry.is_some_and(|e| e.slot_entries.is_empty()) => Some(
            "a granted row with no slots isn't carried by the importer, so its flag is the app's",
        ),
        "level" if group("Temporary_Powers.Accolades") => {
            Some("Mids writes an accolade's level as 1 or 50 and reads neither")
        }
        "level" | "slot 1 level" if group("Inherent.Fitness") => Some(
            "the game data states no grant level for Fitness; Mids reads none on a granted row",
        ),
        "slots" if group("Inherent.Fitness") && difference.mids < difference.ours => Some(
            "Rebirth's free Health and Stamina slots, which Mids' own Mastermind and Widow files carry",
        ),
        "pick row" if group("Inherent.") => Some(
            "Mids sometimes files its own inherent inside the pick rows; it's not a pick either way",
        ),
        what if what.ends_with(" piece")
            && difference.mids.replace(" None", " Even") == difference.ours =>
        {
            Some("Mids wrote relative level None once; the importer reads it as Even")
        }
        "PowerSets" if dedup(&original.power_sets) == dedup(&written.power_sets) => {
            Some("Mids repeats a pool it was re-picked into; the sets are the same")
        }
        _ => None,
    }
}

/// The non-empty entries, each once, in order.
fn dedup(sets: &[String]) -> Vec<&String> {
    let mut seen = BTreeSet::new();
    sets.iter()
        .filter(|set| !set.is_empty() && seen.insert(*set))
        .collect()
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().to_string()
}

#[test]
fn mbd_round_trip_matches_every_fixture() {
    let mut unexplained = Vec::new();
    let mut explained = 0;
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let database = load(dataset);
        for path in fixtures(dataset) {
            let original = mbd::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let (state, declined) = as_the_app_opens(&original, &database, dataset);
            let export = write_mbd(&state, &database, DEFAULT_IO_LEVEL).expect("it writes");
            let reread = mbd::from_str(&export.text()).expect("our own file reads as a .mbd");
            assert_eq!(
                reread,
                export.file,
                "{}: the text says what the file holds",
                name(&path)
            );
            let archetype_inherent = database
                .mids_names
                .as_ref()
                .and_then(|names| names.archetype_inherent(&original.class))
                .map(|name| format!("Inherent.Inherent.{name}"));
            for difference in differences(&original, &export.file) {
                let reason = allowed(
                    &difference,
                    &original,
                    &export.file,
                    &declined,
                    archetype_inherent.as_deref(),
                );
                match reason {
                    Some(_) => explained += 1,
                    None => unexplained.push(format!("{}: {difference:?}", name(&path))),
                }
            }
        }
    }
    for line in &unexplained {
        println!("{line}");
    }
    assert!(
        unexplained.is_empty(),
        "{} unexplained differences",
        unexplained.len()
    );
    println!("{explained} differences, each with its reason");
}

/// MBDEXPORT-19: a slider the player set goes out as the row's `VariableValue`.
#[test]
fn a_slider_goes_out_as_its_rows_variable_value() {
    let dataset = DatasetId::Homecoming;
    let database = load(dataset);
    let path = fixtures(dataset)
        .into_iter()
        .find(|path| name(path).starts_with("warshade-umbral-blast-umbral-aura-v"))
        .expect("the Warshade fixture");
    let original = mbd::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let (mut state, _) = as_the_app_opens(&original, &database, dataset);
    let mire = state
        .primary
        .powers
        .iter_mut()
        .find(|power| power.internal_name == "Sunless_Mire")
        .expect("the build holds Sunless Mire");
    mire.targets_hit = Some(4);
    let export = write_mbd(&state, &database, DEFAULT_IO_LEVEL).expect("it writes");
    let row = export
        .file
        .power_entries
        .iter()
        .find(|entry| entry.power_name.ends_with(".Sunless_Mire"))
        .expect("a Sunless Mire row");
    assert_eq!(row.variable_value, 4);
}
