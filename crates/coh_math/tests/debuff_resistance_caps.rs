//! DDR-CAP-1: the debuff-resistance ceilings come from each archetype's ResistanceMaxTable, and
//! all four forks carry the same curve. Read out of `classes.bin` on 2026-10-27 — the Homecoming
//! wiki's `Limits` table matches defense, to-hit, recovery and regeneration, but not the two
//! things the table adds: endurance reaches 95% eight levels early, and Controller, Dominator
//! and Mastermind regeneration stops at 47.5%.

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

const HALF_REGEN: &[&str] = &["controller", "dominator", "mastermind"];

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn every_fork_carries_the_level_scaled_ceilings() {
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        let archetypes = db.archetypes().expect("archetypes section");
        assert!(!archetypes.all().is_empty(), "{dataset:?}: no archetypes");
        for at in archetypes.all() {
            let caps = db
                .archetype_stats
                .get(&at.id)
                .unwrap_or_else(|| panic!("{dataset:?} {}: no caps", at.id));
            let table = &caps.debuff_resistance_cap_table;
            let at_ = |level| table.at_level(level).expect("50 levels");
            let (l1, l38, l39, l46, l47) = (at_(1), at_(38), at_(39), at_(46), at_(47));
            let who = format!("{dataset:?} {}", at.id);

            for (row, value) in [("defense", l1.defense), ("toHit", l1.to_hit)] {
                assert!(close(value, 20.0), "{who}: {row} at level 1 is {value}");
            }
            assert!(
                close(l46.defense, 93.8),
                "{who}: defense at 46 is {}",
                l46.defense
            );
            assert!(
                close(l47.defense, 95.0),
                "{who}: defense at 47 is {}",
                l47.defense
            );
            assert!(close(l47.recovery, 95.0), "{who}: recovery at 47");

            // Endurance climbs 2 points a level, not 1.64, so it reaches 95 at 39.
            assert!(
                l38.endurance < 95.0,
                "{who}: endurance at 38 is {}",
                l38.endurance
            );
            assert!(
                close(l39.endurance, 95.0),
                "{who}: endurance at 39 is {}",
                l39.endurance
            );

            let regen_top = if HALF_REGEN.contains(&at.id.as_str()) {
                47.5
            } else {
                95.0
            };
            assert!(
                close(at_(50).regeneration, regen_top),
                "{who}: regeneration at 50 is {}",
                at_(50).regeneration
            );

            for (row, value) in [
                ("slow", l1.slow),
                ("recharge", l1.recharge),
                ("perception", l1.perception),
            ] {
                assert!(close(value, 100.0), "{who}: {row} is {value}");
            }
        }
    }
}
