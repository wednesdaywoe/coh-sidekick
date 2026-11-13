//! A `.mbd` older than Mids' UIDs imports the same enhancements as a newer one.
//!
//! Mids 3.6 and earlier name a slotted piece by its long name (`Artillery: Accuracy/Damage`)
//! with no `Uid`. Reported 2026-10-31: such a file imported every power and no enhancement.
//! Each fixture here is rewritten into that older spelling, and must import identically.

use coh_data::mbd::{self, MbdFile};
use coh_data::mbd_import::to_skif_build;
use coh_data::{DatasetId, PowerDatabase};
use std::path::PathBuf;

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

/// The file as Mids 3.6 would have written it: every UID swapped for its long name. Returns how
/// many slots were rewritten; a UID Mids' table does not carry is left as it is.
fn as_pre_uid(file: &mut MbdFile, database: &PowerDatabase) -> usize {
    let names = database.mids_enh_names.as_ref().expect("a Mids name table");
    let mut rewritten = 0;
    for entry in &mut file.power_entries {
        for slot in &mut entry.slot_entries {
            let Some(enhancement) = slot.enhancement.as_mut() else {
                continue;
            };
            let mut carrying = names.records().iter().filter(|r| r.uid == enhancement.uid);
            if let (Some(record), None) = (carrying.next(), carrying.next()) {
                enhancement.enhancement = record.long_name.clone();
                enhancement.uid.clear();
                rewritten += 1;
            }
        }
    }
    rewritten
}

#[test]
fn a_pre_uid_mbd_imports_the_same_enhancements() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let database = load(dataset);
        for path in fixtures(dataset) {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            let file = mbd::from_str(&text).expect("fixture parses");
            let mut old = file.clone();
            let rewritten = as_pre_uid(&mut old, &database);

            let current = to_skif_build(&file, &database, dataset);
            let legacy = to_skif_build(&old, &database, dataset);
            assert_eq!(legacy.build, current.build, "{path:?}");
            assert_eq!(
                legacy.summary.enhancements_imported, current.summary.enhancements_imported,
                "{path:?}"
            );
            if current.summary.enhancements_imported > 0 {
                assert!(rewritten > 0, "{path:?} rewrote nothing");
            }
        }
    }
}

#[test]
fn a_long_name_mids_does_not_carry_is_reported() {
    let database = load(DatasetId::Homecoming);
    let path = fixtures(DatasetId::Homecoming).remove(0);
    let text = std::fs::read_to_string(&path).expect("fixture reads");
    let mut file = mbd::from_str(&text).expect("fixture parses");
    let enhancement = file
        .power_entries
        .iter_mut()
        .flat_map(|entry| entry.slot_entries.iter_mut())
        .find_map(|slot| slot.enhancement.as_mut())
        .expect("a slotted piece");
    enhancement.uid.clear();
    enhancement.enhancement = "Artillery: Nothing At All".to_string();

    let converted = to_skif_build(&file, &database, DatasetId::Homecoming);
    assert!(converted
        .unresolved
        .iter()
        .any(|note| note.context.starts_with("Artillery: Nothing At All")));
    assert!(converted.summary.reconciles());
}
