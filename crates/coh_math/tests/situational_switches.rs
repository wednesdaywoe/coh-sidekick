//! A situational damage component the player can answer joins the hit once they have.
//!
//! Propel carries both kinds. Its Impact rider asks `now GravityDistortion target.TokenTime> - 12
//! <`, which no build state says, and the export ties it to the power's own "Impact (Gravity
//! Distortion)" switch; its Containment twin asks whether the target is held, stunned, slept,
//! immobilized or afraid, which the target's `held` answers. Before either was read, both sat in
//! "only in some situations" whatever the switches said, and the Impact row had no name.
//!
//! Each half has its control: with the switch off and the target's state unstated, both stay
//! situational, so an assertion that passed by summing every gated row would fail there.

use coh_data::{DatasetId, Power, PowerDatabase};
use coh_math::damage::{resolve_power_damage, PowerDamage};
use coh_math::expr::{SourceContext, TargetIdentity};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn propel(db: &PowerDatabase) -> &Power {
    db.powersets
        .iter()
        .filter(|set| set.id.contains("controller") && set.id.contains("gravity"))
        .flat_map(|set| set.powers.iter())
        .find(|p| p.internal_name.as_deref() == Some("Propel"))
        .expect("a Controller's Propel")
}

fn damage(db: &PowerDatabase, held: bool, impact_on: bool) -> PowerDamage {
    let ctx = SourceContext {
        target: Some(TargetIdentity {
            archetype_class: None,
            entity_type: "critter".to_string(),
            modes: Default::default(),
            held,
        }),
        caster_class: db.class_name_of("controller").map(str::to_string),
        target_alive: true,
        active_conditionals: if impact_on {
            ["gravitydistortion".to_string()].into()
        } else {
            Default::default()
        },
        ..Default::default()
    };
    let mut errors = Vec::new();
    let damage = resolve_power_damage(
        propel(db),
        "controller",
        50,
        0.0,
        0.0,
        &ctx,
        db,
        &mut errors,
    );
    assert!(errors.is_empty(), "{errors:?}");
    damage
}

fn table(db: &PowerDatabase, name: &str) -> f64 {
    db.at_tables
        .get_table_value("controller", name, 50)
        .unwrap_or_else(|| panic!("controller has no {name}"))
        .abs()
}

#[test]
fn propel_counts_impact_and_containment_once_answered() {
    let db = load(DatasetId::Homecoming);
    let hit = 1.96 * table(&db, "Ranged_Damage");
    let impact = 0.49 * table(&db, "Ranged_Damage");
    let containment = 1.96 * table(&db, "Ranged_InherentDamage");

    // Nobody has said anything: the base hit only, both riders situational.
    let quiet = damage(&db, false, false);
    assert!((quiet.base - hit).abs() < 0.01, "{}", quiet.base);
    assert_eq!(quiet.unresolved.len(), 2, "{:?}", quiet.unresolved);
    let waiting: Vec<bool> = quiet.unresolved.iter().map(|u| u.waits_on_held).collect();
    assert_eq!(
        waiting.iter().filter(|w| **w).count(),
        1,
        "only Containment waits on the held target: {:?}",
        quiet.unresolved
    );
    let situational = quiet
        .unresolved
        .iter()
        .find(|u| !u.waits_on_held)
        .and_then(|u| u.if_it_lands.as_ref())
        .expect("Impact valued while it waits");
    assert_eq!(
        situational.condition.as_deref(),
        Some("Impact (Gravity Distortion)")
    );

    // The switch on: Impact joins the hit, named.
    let switched = damage(&db, false, true);
    assert!(
        (switched.base - (hit + impact)).abs() < 0.01,
        "{}",
        switched.base
    );
    assert!(switched
        .components
        .iter()
        .any(|c| c.condition.as_deref() == Some("Impact (Gravity Distortion)")));

    // And the target held: Containment joins too, and nothing is left waiting.
    let both = damage(&db, true, true);
    assert!(
        (both.base - (hit + impact + containment)).abs() < 0.01,
        "{}",
        both.base
    );
    assert!(both.unresolved.is_empty(), "{:?}", both.unresolved);
}

/// The held answer never reaches the player-only twin: it is the PvE/PvP fork that fails it, and
/// lifting an Unknown cannot touch a gate that already Fails.
#[test]
fn held_target_adds_no_pvp_component() {
    let db = load(DatasetId::Homecoming);
    let both = damage(&db, true, true);
    assert!(
        both.components.iter().all(|c| !c.table.contains("PvP")),
        "{:?}",
        both.components
    );
}
