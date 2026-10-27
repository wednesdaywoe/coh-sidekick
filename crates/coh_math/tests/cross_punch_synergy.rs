//! Cross Punch's self +ToHit and +Recharge is 5% for EACH of Boxing and Kick the build owns
//! (`@StdResult × (Kick owned + Boxing owned)` in both forks' data), per stack. It used to read a
//! flat 5% whatever the build held, even with neither partner.

use coh_data::{CharacterState, DatasetId, PoolSelection, PowerDatabase, SelectedPower};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Total +ToHit and +Recharge (percent) for a level-50 Blaster holding `picks` from Fighting,
/// with Cross Punch switched on at `stacks` stacks.
fn totals(db: &PowerDatabase, dataset: DatasetId, picks: &[&str], stacks: u32) -> (f64, f64) {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("blaster".into());
    state.pools = vec![PoolSelection {
        id: "fighting".into(),
        name: "fighting".into(),
        powers: picks
            .iter()
            .map(|name| {
                let mut pick = SelectedPower::picked(name.to_string(), "fighting", 1);
                if *name == "Cross_Punch" {
                    pick.is_active = true;
                    pick.targets_hit = Some(stacks);
                }
                pick
            })
            .collect(),
    }];
    let t = coh_math::recalculate(&state, db);
    (t.bonuses.to_hit, t.bonuses.recharge)
}

#[test]
fn cross_punch_buff_counts_boxing_and_kick() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let db = load(dataset);
        for (picks, partners) in [
            (&["Boxing", "Kick", "Cross_Punch"][..], 2.0),
            (&["Boxing", "Cross_Punch"][..], 1.0),
            (&["Kick", "Cross_Punch"][..], 1.0),
            (&["Tough", "Cross_Punch"][..], 0.0),
        ] {
            for stacks in [1, 2] {
                let want = 5.0 * partners * stacks as f64;
                let (to_hit, recharge) = totals(&db, dataset, picks, stacks);
                assert!(
                    (to_hit - want).abs() < 1e-6 && (recharge - want).abs() < 1e-6,
                    "{dataset:?} {picks:?} at {stacks} stack(s): ToHit {to_hit}%, Recharge \
                     {recharge}%, expected {want}% each"
                );
            }
        }
    }
}

/// The partners raise Cross Punch's buff, not the build's: owning them with Cross Punch
/// switched off, or not taken, adds nothing.
#[test]
fn boxing_and_kick_alone_grant_no_buff() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let db = load(dataset);
        assert_eq!(
            totals(&db, dataset, &["Boxing", "Kick"], 1),
            (0.0, 0.0),
            "{dataset:?}"
        );
    }
}
