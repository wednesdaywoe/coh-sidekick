//! Proc potential: the base-recharge proc pool a power could host, and the badge tier it earns.
//!
//! The anchors are the beta's `proc-potential.test.ts`, power for power, so a port that drifted
//! from the beta's join or geometry fails here by name. Both failure modes produce plausible
//! numbers, which is why the counts are exact.

use coh_data::{DatasetId, PowerDatabase};
use coh_math::proc_potential::{proc_potential, ProcPotential, ProcTier};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../contract/{}/bundle.json.gz",
        dataset.as_str()
    ));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("load bundle")
}

fn potential(db: &PowerDatabase, set: &str, power: &str) -> ProcPotential {
    let def = db
        .find_power(set, power)
        .unwrap_or_else(|| panic!("{set} {power} not in the bundle"));
    proc_potential(def, db).unwrap_or_else(|| panic!("{power} accepts no IO set"))
}

fn capped_in(p: &ProcPotential, category: &str) -> usize {
    p.composition
        .iter()
        .find(|r| r.category == category)
        .map_or(0, |r| r.at_cap)
}

#[test]
fn ice_arrow_is_the_breadth_case() {
    // 18s, single target: the Holds ∪ Slow union pulls in damage procs a pure control power
    // can't reach. Its 2.5 and 2 PPM procs stay under the ceiling at 18s.
    let db = load(DatasetId::Homecoming);
    let p = potential(&db, "defender/trick-arrow", "Ice_Arrow");
    assert_eq!((p.entries.len(), p.at_cap), (11, 9));
    assert_eq!(p.area_factor, 1.0);
    assert_eq!(p.purple_sets, ["Unbreakable Constraint"]);
    let mut below: Vec<f64> = p
        .entries
        .iter()
        .filter(|e| !e.at_cap)
        .map(|e| e.ppm)
        .collect();
    below.sort_by(f64::total_cmp);
    assert_eq!(below, [2.0, 2.5]);
    assert_eq!(p.tier(), ProcTier::ProcBomb);
}

#[test]
fn dark_consumption_caps_everything_despite_its_aoe() {
    // The 8ft AoE tax is real (÷1.9) and 180s of recharge overwhelms it, down to the 1.5 PPM
    // Performance Shifter. Its knockdown and -Res procs cap too, not just damage.
    let db = load(DatasetId::Homecoming);
    let p = potential(&db, "scrapper/dark-melee", "Dark_Consumption");
    assert_eq!((p.entries.len(), p.at_cap), (12, 12));
    assert!((p.area_factor - 1.9).abs() < 1e-3, "{}", p.area_factor);
    assert_eq!(p.window, 180.0);
    assert_eq!(
        (
            capped_in(&p, "Control"),
            capped_in(&p, "Damage"),
            capped_in(&p, "Debuff")
        ),
        (5, 4, 1)
    );
    assert!(p.global_count > 0);
    assert_eq!(p.tier(), ProcTier::Exceptional);
}

#[test]
fn a_fast_attack_caps_nothing_however_large_its_pool() {
    let db = load(DatasetId::Homecoming);
    let p = potential(&db, "scrapper/dark-melee", "Shadow_Punch");
    assert_eq!((p.entries.len(), p.at_cap), (14, 0));
    assert!(p.entries.iter().all(|e| e.chance < 0.3));
    assert_eq!(p.tier(), ProcTier::Unremarkable);
}

#[test]
fn seismic_smash_is_exceptional() {
    let db = load(DatasetId::Homecoming);
    let p = potential(&db, "tanker/stone-melee", "Seismic_Smash");
    assert_eq!(p.at_cap, 15);
    assert_eq!(p.tier(), ProcTier::Exceptional);
}

#[test]
fn a_rain_rolls_on_its_patch_and_main_target_only_drops_the_aoe_tax() {
    let db = load(DatasetId::Homecoming);
    let rain = potential(&db, "corruptor/fire-blast", "Rain_of_Fire");
    assert!(rain.from_patch);
    assert!(rain.area_factor > 3.0, "{}", rain.area_factor);
    assert_eq!(rain.tier(), ProcTier::Unremarkable);

    let propel = potential(&db, "dominator/gravity-control", "Propel");
    assert!(propel.main_target_only);
    assert_eq!(propel.area_factor, 1.0);
}

#[test]
fn the_badge_stays_rare_on_every_fork() {
    // A badge on half the catalogue is decoration. The beta calibrated ~15% on Homecoming; the
    // bounds survive a regen and catch a threshold slip.
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        let (mut n, mut badged) = (0usize, 0usize);
        for def in db.all_powers() {
            let Some(p) = proc_potential(def, &db) else {
                continue;
            };
            n += 1;
            if p.tier() != ProcTier::Unremarkable {
                badged += 1;
            }
            let mut ids: Vec<_> = p
                .entries
                .iter()
                .map(|e| (&e.set_name, &e.io_name))
                .collect();
            let all = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), all, "{}: a proc listed twice", def.name);
        }
        let pct = badged as f64 * 100.0 / n as f64;
        println!("{dataset:?}: {badged}/{n} badged ({pct:.1}%)");
        assert!(n > 2000, "{dataset:?}: only {n} powers with a proc pool");
        assert!((8.0..25.0).contains(&pct), "{dataset:?}: {pct:.1}% badged");
    }
}
