//! Shopping and crafting progress rides a saved build file, and stays out of a share: it is the
//! owner's inventory, not the build.

use coh_data::{skif, CharacterState, DatasetId, PowerDatabase};
use std::path::PathBuf;

fn homecoming() -> PowerDatabase {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/homecoming/bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
}

fn shopping() -> CharacterState {
    let mut build = CharacterState::empty(DatasetId::Homecoming);
    build
        .enhancement_acquired
        .insert("generic:Defense|50|0|0".into(), 3);
    build
        .shopping_acquired
        .insert("S_DetailedReports".into(), 2);
    build.crafting_obtained.insert("alpha:node".into());
    build
        .crafting_salvage_checked
        .insert("alpha:node:S_DetailedReports".into());
    build
}

#[test]
fn a_saved_file_brings_the_progress_back() {
    let database = homecoming();
    let saved = shopping();
    let text = skif::encode(&saved, &database).expect("encodes");
    let back = skif::decode(&text, &database).expect("decodes").build;
    assert_eq!(back.enhancement_acquired, saved.enhancement_acquired);
    assert_eq!(back.shopping_acquired, saved.shopping_acquired);
    assert_eq!(back.crafting_obtained, saved.crafting_obtained);
    assert_eq!(
        back.crafting_salvage_checked,
        saved.crafting_salvage_checked
    );
}

#[test]
fn a_share_carries_none_of_it() {
    let database = homecoming();
    let text = skif::encode_shared(&shopping(), &database).expect("encodes");
    assert!(!text.contains("progress"), "{text}");
    let back = skif::decode(&text, &database).expect("decodes").build;
    assert!(back.enhancement_acquired.is_empty());
    assert!(back.shopping_acquired.is_empty());
}

/// The beta wrote its salvage ids without the export's `S_` prefix.
#[test]
fn a_beta_files_shopping_counts_come_across() {
    let database = homecoming();
    let text = r#"{
      "version": 4,
      "build": {
        "serverId": "homecoming",
        "level": 50,
        "shoppingListAcquired": { "DetailedReports": 2, "NotASalvage": 1 },
        "craftingChecklist": { "alpha:vigor:1:common:DetailedReports": true }
      }
    }"#;
    let decoded = skif::legacy::decode_legacy(text, &database).expect("decodes");
    let acquired = &decoded.build.shopping_acquired;
    assert_eq!(acquired.get("S_DetailedReports"), Some(&2), "{acquired:?}");
    assert_eq!(acquired.len(), 1);
    let said = |needle: &str| {
        decoded
            .unresolved
            .iter()
            .any(|note| note.context.contains(needle))
    };
    assert!(said("NotASalvage"), "{:?}", decoded.unresolved);
    assert!(said("crafting checklist"), "{:?}", decoded.unresolved);
}
