//! The what-if team-buff layer survives this browser's own autosave, so a user's simulated buffs
//! come back after the window closes, and still never reaches a build file or a share.

use coh_data::{build_store, skif, CharacterState, DatasetId, StoredBuilds};
use std::collections::BTreeMap;

fn simulating() -> CharacterState {
    let mut build = CharacterState::empty(DatasetId::Homecoming);
    build.level = 50;
    build.combat.what_if_buffs.insert("recharge".into(), 25.0);
    build
}

#[test]
fn the_autosave_round_trip_keeps_the_layer() {
    let empty = StoredBuilds {
        active_dataset: DatasetId::Homecoming,
        builds_by_dataset: BTreeMap::new(),
    };
    let saved = build_store::compose(&simulating(), &empty);
    let text = build_store::encode(&saved).expect("encode");
    let read = build_store::decode(&text).expect("decode");
    let restored = build_store::select_active(&read, DatasetId::Homecoming);
    assert_eq!(restored.combat.what_if_buffs.get("recharge"), Some(&25.0));
}

#[test]
fn a_build_file_never_carries_the_layer() {
    let file = skif::slim(&simulating()).expect("slim");
    assert!(file.what_if_buffs.is_none());
}
