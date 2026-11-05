//! A foe −Damage-Resistance carried by a summoned entity must reach the planner.
//!
//! The row is authored as the damage TYPES themselves at `aspect: Resistance` with a negative
//! scale — `Smashing_Dmg, Lethal_Dmg, … Toxic_Dmg` at −2 on `Ranged_Res_Dmg`. The ASPECT is what
//! makes it a resistance rather than damage, and no name-keyed table in the converter can hold
//! that: `DEBUFF_ATTRIBS` keys position and resource attribs and deliberately holds no damage
//! type, the buff-aura pass wants a positive scale, and the damage-strength pass wants
//! `aspect: Strength`. The row reached the last branch it could reach and there was none, so it
//! parsed, atomized and fell out — Thunderspy had the rule and the other three forks did not.
//!
//! Acid Mortar is the report. Its own shipped text says the mortar "reduces the target's Defense
//! as well as their Damage Resistance", and the Info panel EFFECTS read `-Defense 20%` alone.

use coh_data::{DatasetId, Power, PowerDatabase};
use coh_math::granted::pseudo_pet_effects;
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn powers<'a>(db: &'a PowerDatabase, ident: &str) -> Vec<&'a Power> {
    let found: Vec<&Power> = db.all_powers().filter(|p| p.ident() == ident).collect();
    assert!(!found.is_empty(), "no power {ident}");
    found
}

/// The mortar's −Res, on every fork. Thunderspy already had it, so it is the control that says
/// the other three were missing a rule rather than missing the row.
#[test]
fn acid_mortars_pet_lands_its_damage_resistance_debuff() {
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        for power in powers(&db, "Acid_Mortar") {
            let bag = pseudo_pet_effects(power, &db);
            assert!(
                bag.contains_key("resistanceDebuff"),
                "{dataset:?} Acid Mortar: the pet's −Res never reached the summoner's bag \
                 (keys: {:?})",
                bag.keys().collect::<Vec<_>>()
            );
            // The shipped power states both halves together, so a bag carrying one and not the
            // other is a route that lost one, whatever the numbers say.
            assert!(
                bag.contains_key("defenseDebuff"),
                "{dataset:?} Acid Mortar: −Def went missing beside −Res \
                 (keys: {:?})",
                bag.keys().collect::<Vec<_>>()
            );
        }
    }
}

/// The row is a foe debuff, so it publishes under the debuff name and never as the pet's own
/// +Resistance — the same sign trap that keeps a pet's `Res_DMG +N` self-buff out of the foe
/// column. A bag that gained `resistanceDebuff` by reading the buff direction upwards would
/// still pass the test above.
#[test]
fn a_foe_resistance_row_is_not_the_pets_own_resistance() {
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        for power in powers(&db, "Acid_Mortar") {
            let bag = pseudo_pet_effects(power, &db);
            let row = &bag["resistanceDebuff"];
            assert!(
                row.get("scale")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0)
                    > 0.0,
                "{dataset:?} Acid Mortar: the −Res row lost its magnitude: {row}"
            );
            assert!(
                !bag.contains_key("resistanceBuff"),
                "{dataset:?} Acid Mortar: the foe's −Res surfaced as an ally +Res \
                 (keys: {:?})",
                bag.keys().collect::<Vec<_>>()
            );
        }
    }
}

/// A `*_Ones` row is a marker, not a computable percent: a pet's own `Resistance` power lists a
/// full immunity that way, at −10 across every damage type and `target: Self`. Reading those as
/// foe −Res would advertise an immunity as a debuff.
#[test]
fn a_ones_table_marker_is_not_a_foe_resistance_debuff() {
    for dataset in DatasetId::ALL {
        let db = load(dataset);
        for power in powers(&db, "Acid_Mortar") {
            let bag = pseudo_pet_effects(power, &db);
            let Some(row) = bag.get("resistanceDebuff") else {
                continue;
            };
            let table = row.get("table").and_then(serde_json::Value::as_str);
            assert!(
                !table
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .ends_with("_ones"),
                "{dataset:?} Acid_Mortar surfaced a `*_Ones` marker as a foe −Res: {row}"
            );
        }
    }
}
