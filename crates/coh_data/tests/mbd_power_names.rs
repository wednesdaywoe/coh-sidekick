//! Every power name the Mids writer can produce, on every fork, against Mids' own power list.
//!
//! Mids resolves a `PowerName` with an exact, case-sensitive compare and answers a miss with a
//! blank row that keeps its slots, so a near miss is the bug (MBDEXPORT-3, -8, -23, -24). A
//! power Mids simply doesn't have is a fork difference, reported but not failed.

use coh_data::mbd_export::{mids_full_name, mids_set_path};
use coh_data::{DatasetId, PowerDatabase, ACCOLADE_CATEGORY, INHERENT_SET};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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

/// Mids' powers by its literal set path, from the dump `convert-mids-name-map.cjs` reads.
/// Brainstorm builds go out in Mids' Homecoming database, so they're checked against it.
fn mids_powers(dataset: DatasetId) -> BTreeMap<String, BTreeSet<String>> {
    let names_from = match dataset {
        DatasetId::Brainstorm => DatasetId::Homecoming,
        other => other,
    };
    let path = root().join(format!(
        "tools/mids-oracle/mids-power-names.{}.json",
        names_from.as_str()
    ));
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    dump["powersets"]
        .as_object()
        .expect("a powersets map")
        .iter()
        .map(|(set, powers)| {
            let names = powers
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|row| row[0].as_str().map(str::to_string))
                .collect();
            (set.clone(), names)
        })
        .collect()
}

fn fold(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Every full name the writer can produce on this fork, each with what it's for.
fn written_names(database: &PowerDatabase) -> Vec<(String, String)> {
    let names = database.mids_names.as_ref().expect("a Mids name map");
    let mut out = Vec::new();
    let mut add = |what: String, path: &str, power: &str| {
        out.push((what, mids_full_name(names, path, power)));
    };

    let catalog = &database.pool_catalog;
    let sets = database
        .powersets
        .iter()
        .filter(|set| set.category.as_deref() != Some(ACCOLADE_CATEGORY) && set.id != INHERENT_SET)
        .filter_map(|set| {
            Some((
                set.id.as_str(),
                set.set_path.as_deref()?,
                set.powers.iter().collect::<Vec<_>>(),
            ))
        })
        .chain(
            catalog
                .pools
                .iter()
                .chain(catalog.epics.iter())
                .filter_map(|pool| {
                    let powers = database
                        .pool_powers
                        .iter()
                        .chain(database.epic_powers.iter())
                        .filter(|entry| entry.set_id == pool.id)
                        .map(|entry| &entry.power)
                        .collect();
                    Some((pool.id.as_str(), pool.set_path.as_deref()?, powers))
                }),
        );
    let inherent_path = database
        .find_powerset(INHERENT_SET)
        .and_then(|set| set.set_path.clone())
        .expect("the inherent set has a path");
    for (id, path, powers) in sets {
        for power in powers {
            match power.is_auto_issued() {
                // Granted ones with slots go out under the inherent set (MBDEXPORT-5).
                true if power.max_slots().unwrap_or(0) > 0 => {
                    add(format!("granted in {id}"), &inherent_path, power.ident())
                }
                true => {}
                false => add(format!("pick in {id}"), path, power.ident()),
            }
        }
    }

    if let Ok(Some(grants)) = database.inherent_grants() {
        for grant in &grants.grants {
            if let Some((path, power)) = grant.full_name.as_deref().and_then(|f| f.rsplit_once('.'))
            {
                add("inherent".into(), path, power);
            }
        }
    }
    for (set_id, power) in database.accolade_powers_with_set() {
        if let Some(path) = database
            .find_powerset(set_id)
            .and_then(|s| s.set_path.as_deref())
        {
            add("accolade".into(), path, power.ident());
        }
    }
    for slot in &database.incarnate_catalog.slots {
        for power in &slot.powers {
            add(
                format!("incarnate {}", slot.id),
                &slot.key,
                &power.internal_name,
            );
        }
    }
    // The archetype inherent goes out under the name Mids shows for the class (MBDEXPORT-24).
    if let Ok(archetypes) = database.archetypes() {
        for archetype in archetypes.all() {
            let Some(class) = database.class_name_of(&archetype.id) else {
                continue;
            };
            if let Some(name) = names.archetype_inherent(class) {
                let path = mids_set_path(names, &inherent_path);
                out.push((format!("{class} inherent"), format!("{path}.{name}")));
            }
        }
    }
    out
}

#[test]
fn every_written_power_name_is_one_mids_knows() {
    let mut near_misses = Vec::new();
    for dataset in DatasetId::ALL {
        let database = load(dataset);
        let mids = mids_powers(dataset);
        let (mut exact, mut set_absent, mut power_absent) = (0, 0, 0);
        let mut absent_sets = BTreeSet::new();
        for (what, full) in written_names(&database) {
            let Some((set, power)) = full.rsplit_once('.') else {
                near_misses.push(format!("{dataset:?} {what}: {full:?} has no set"));
                continue;
            };
            let Some(powers) = mids.get(set) else {
                set_absent += 1;
                absent_sets.insert(set.to_string());
                continue;
            };
            if powers.contains(power) {
                exact += 1;
                continue;
            }
            let close: Vec<&String> = powers.iter().filter(|p| fold(p) == fold(power)).collect();
            match close.as_slice() {
                [] => power_absent += 1,
                _ => near_misses.push(format!(
                    "{dataset:?} {what}: we write {full}, Mids has {close:?}"
                )),
            }
        }
        println!(
            "{dataset:?}: {exact} names Mids has, {set_absent} in a set Mids lacks, \
             {power_absent} powers Mids' set lacks"
        );
        println!("  sets Mids lacks: {absent_sets:?}");
        assert!(
            exact > 1000,
            "{dataset:?}: only {exact} names matched; the dump or the map is off"
        );
    }
    for line in &near_misses {
        println!("{line}");
    }
    assert!(near_misses.is_empty(), "{} near misses", near_misses.len());
}
