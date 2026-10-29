//! A VEAT read from a file holds its base sets in the primary and secondary seats.
//!
//! Mids files a Fortunata under `Widow_Training.Fortunata_Training`, so an import used to seat
//! the branch pair, and a build saved after that kept it. The rail draws the seated set and the
//! held branch, so it drew Fortunata twice and never offered Widow Training or Teamwork.

use coh_data::{CharacterState, DatasetId, PowerDatabase};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// The shape the bug report's file had: branch sets seated, base picks naming their own set.
const SEATED_BRANCH: &str = r#"{
  "version": 5,
  "build": {
    "name": "seated branch",
    "dataset": "homecoming",
    "archetype": "arachnos-widow",
    "level": 50,
    "primary": {
      "id": "arachnos-widow/fortunata-training",
      "powers": [
        { "internalName": "FRT_Telekinetic_Blast", "level": 1, "slots": [] },
        { "internalName": "Lunge", "powerset": "arachnos-widow/widow-training", "level": 2, "slots": [] }
      ]
    },
    "secondary": {
      "id": "arachnos-widow/fortunata-teamwork",
      "powers": [
        { "internalName": "Fate_Sealed", "level": 1, "slots": [] },
        { "internalName": "Foresight", "powerset": "arachnos-widow/teamwork", "level": 22, "slots": [] }
      ]
    }
  }
}"#;

fn set_of<'a>(build: &'a CharacterState, name: &str) -> &'a str {
    build
        .all_selected()
        .find(|power| power.internal_name == name)
        .unwrap_or_else(|| panic!("{name} was read"))
        .powerset
        .as_str()
}

fn assert_base_seated(build: &CharacterState) {
    assert_eq!(
        build.primary.id.as_deref(),
        Some("arachnos-widow/widow-training")
    );
    assert_eq!(
        build.secondary.id.as_deref(),
        Some("arachnos-widow/teamwork")
    );
}

#[test]
fn a_seated_branch_reads_back_under_the_base_set() {
    let db = load(DatasetId::Homecoming);
    let decoded = coh_data::skif::decode(SEATED_BRANCH, &db).expect("the file decodes");
    assert!(decoded.unresolved.is_empty(), "{:?}", decoded.unresolved);
    let build = decoded.build;
    assert_base_seated(&build);
    // Each pick keeps its own set, so the branch is still the one held.
    assert_eq!(
        set_of(&build, "FRT_Telekinetic_Blast"),
        "arachnos-widow/fortunata-training"
    );
    assert_eq!(set_of(&build, "Lunge"), "arachnos-widow/widow-training");
    assert_eq!(
        set_of(&build, "Fate_Sealed"),
        "arachnos-widow/fortunata-teamwork"
    );
    assert_eq!(set_of(&build, "Foresight"), "arachnos-widow/teamwork");

    // And a save of it stays that way.
    let text = coh_data::skif::encode(&build, &db).expect("the build encodes");
    let again = coh_data::skif::decode(&text, &db).expect("its own file decodes");
    assert_base_seated(&again.build);
    assert_eq!(
        set_of(&again.build, "FRT_Telekinetic_Blast"),
        "arachnos-widow/fortunata-training"
    );
}

/// The corpus Night Widow, as Mids wrote it: `Widow_Training.Night_Widow_Training` first.
#[test]
fn a_mids_widow_imports_under_the_base_sets() {
    let db = load(DatasetId::Rebirth);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mids/rebirth/veat-night-widow-teamwork-v3860.mbd");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let file = coh_data::mbd::from_str(&text).expect("fixture parses");
    let converted = coh_data::mbd_import::to_skif_build(&file, &db, DatasetId::Rebirth);
    let decoded = coh_data::skif::hydrate(
        coh_data::skif::SkifFile {
            version: coh_data::skif::VERSION,
            authored_against: None,
            meta: None,
            build: converted.build,
        },
        &db,
    )
    .expect("the import hydrates");
    let build = decoded.build;
    assert_base_seated(&build);
    let branch_picks = build
        .all_selected()
        .filter(|power| {
            power.powerset == "arachnos-widow/night-widow-training"
                || power.powerset == "arachnos-widow/widow-teamwork"
        })
        .count();
    assert!(branch_picks > 0, "the branch picks kept their own set");
}
