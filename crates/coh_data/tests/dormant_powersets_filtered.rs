//! Unreleased primaries and secondaries stay out of the archetype rosters.
//!
//! Wind Control sits in Homecoming's bins behind a dev-only `accesslevel > 0` gate, so the
//! game won't let a player pick it; Rebirth and Thunderspy have released it. The powerset is
//! stamped `dormant: true` where it's unreleased, and the catalog has to honour that, or the
//! picker offers a set the character creator refuses (user report, 2026-11-25).

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

fn offers(db: &PowerDatabase, archetype: &str, set: &str) -> bool {
    let catalog = db.archetypes().expect("archetypes decode");
    let at = catalog.get(archetype).expect("archetype exists");
    at.primary_sets.iter().any(|id| id == set)
}

const WIND: [(&str, &str); 2] = [
    ("controller", "controller/wind-control"),
    ("dominator", "dominator/wind-control"),
];

#[test]
fn homecoming_and_brainstorm_hide_wind_control() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        for (at, set) in WIND {
            // The set still loads (old builds and the calc can find it) …
            let ps = db.find_powerset(set).expect("set still in the bundle");
            assert!(ps.dormant, "{dataset:?} {set} should be dormant");
            // … but no roster offers it.
            assert!(!offers(&db, at, set), "{dataset:?} {at} offers {set}");
        }
        // Anchor: a released set survives the filter.
        assert!(offers(&db, "dominator", "dominator/fire-control"));
    }
}

#[test]
fn rebirth_and_thunderspy_still_offer_wind_control() {
    for dataset in [DatasetId::Rebirth, DatasetId::Thunderspy] {
        let db = load(dataset);
        for (at, set) in WIND {
            if db.find_powerset(set).is_none() {
                continue; // this fork doesn't give that AT the set at all
            }
            assert!(offers(&db, at, set), "{dataset:?} {at} lost {set}");
        }
    }
}

#[test]
fn no_roster_lists_a_dormant_set() {
    for dataset in [
        DatasetId::Homecoming,
        DatasetId::Rebirth,
        DatasetId::Thunderspy,
        DatasetId::Brainstorm,
    ] {
        let db = load(dataset);
        let catalog = db.archetypes().expect("archetypes decode");
        for at in catalog.all() {
            for id in at.primary_sets.iter().chain(&at.secondary_sets) {
                let dormant = db.find_powerset(id).is_some_and(|ps| ps.dormant);
                assert!(!dormant, "{dataset:?} {} lists dormant {id}", at.id);
            }
        }
    }
}
