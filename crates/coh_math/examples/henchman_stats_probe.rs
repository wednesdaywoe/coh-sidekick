//! Committed probe — worklog henchman-stats-1. What a Mastermind henchman's OWN stats
//! project to from the shipped bundle, for the main-tab/pet-tab split mock (item 2) and
//! the owner's gate (item 3).
//!
//! Reads the entity's abilities at its OWN character class (`TableScope::Pet`, the ENT-10
//! reading buff_pets uses), at level 50. The henchman level offset is an open modeling
//! question (henchman-owner-buffs out-of-scope note): in game a minion may sit below the
//! Mastermind's level; this probe states the level it reads at and does not guess an offset.
//!
//! The DPS figure is a SINGLE-ABILITY rotation proxy — per-activation damage over the
//! ability's recharge — not a simulated AI rotation. It is the honest cheap basis for a
//! damage meter; a full rotation model is a successor-plan decision.
//!
//! Run: cargo run -p coh_math --example henchman_stats_probe

use coh_data::{DatasetId, EffectType, PowerDatabase, SelectedPower, TableScope};
use coh_math::granted::{active_entity_abilities, mastermind_henchman};
use coh_math::scaled::resolve_scaled_effect_for;
use coh_math::totals::CalcError;
use serde_json::Value;
use std::path::PathBuf;

const LEVEL: i32 = 50;
const FORKS: [DatasetId; 4] = [
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Thunderspy,
    DatasetId::Brainstorm,
];

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn resolve(scale: f64, table: Option<&str>, class: &str, db: &PowerDatabase) -> f64 {
    let mut errors: Vec<CalcError> = vec![];
    let v = resolve_scaled_effect_for(scale, table, TableScope::Pet(class), LEVEL, db, &mut errors);
    assert!(errors.is_empty(), "table resolve errors: {errors:?}");
    v
}

fn types_suffix(effect: &Value) -> String {
    for key in ["resistanceTypes", "mezTypes", "defenseTypes", "damageTypes"] {
        if let Some(arr) = effect.get(key).and_then(Value::as_array) {
            let names: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
            return format!(" [{}]", names.join(","));
        }
    }
    String::new()
}

fn print_loadout(entity: &Value, class: &str, db: &PowerDatabase, label: &str, holds: &[&str]) {
    let abilities = active_entity_abilities(entity, &|name| holds.contains(&name));
    println!("  --- {label} ({} abilities) ---", abilities.len());
    for a in abilities {
        let name = a.get("displayName").and_then(Value::as_str).unwrap_or("?");
        let internal = a.get("name").and_then(Value::as_str).unwrap_or("?");
        let kind = a.get("type").and_then(Value::as_str).unwrap_or("?");
        let damage = a.get("damage").and_then(Value::as_array);
        if let Some(components) = damage.filter(|c| !c.is_empty()) {
            let mut total = 0.0;
            let mut hits = Vec::new();
            for c in components {
                let scale = c.get("scale").and_then(Value::as_f64).unwrap_or(0.0);
                let table = c.get("table").and_then(Value::as_str);
                let ty = c.get("damageType").and_then(Value::as_str).unwrap_or("?");
                // CoH damage tables store negative magnitudes (damage is a negative HP delta).
                let per_hit = resolve(scale, table, class, db).abs();
                total += per_hit;
                hits.push(format!("{ty} {per_hit:.2}"));
            }
            let recharge = a.get("recharge").and_then(Value::as_f64).unwrap_or(0.0);
            let cast = a.get("castTime").and_then(Value::as_f64).unwrap_or(0.0);
            let cycle = recharge.max(cast);
            let dps = if cycle > 0.0 { total / cycle } else { 0.0 };
            println!(
                "    ATTACK {name} ({internal}, {kind}): {} hit(s) [{}], {total:.2}/activation, rech {recharge}s cast {cast}s -> {dps:.2} dmg/s proxy",
                components.len(),
                hits.join(" + ")
            );
        }
        if let Some(effects) = a.get("effects").and_then(Value::as_array) {
            for e in effects {
                let ty = e.get("type").and_then(Value::as_str).unwrap_or("?");
                let scale = e.get("scale").and_then(Value::as_f64).unwrap_or(0.0);
                let table = e.get("table").and_then(Value::as_str);
                let v = resolve(scale, table, class, db);
                let pct = if ty.contains("Resistance") || ty.contains("Defense") {
                    format!(" ({:.2}%)", v.abs() * 100.0)
                } else {
                    String::new()
                };
                println!("    EFFECT {ty}: {v:.3}{pct}{}", types_suffix(e));
            }
        }
    }
}

fn roster(db: &PowerDatabase) {
    println!("  ROSTER (EntCreate magnitude = summon count):");
    for summon in ["Soldiers", "Spec_Ops", "Medic", "Commando"] {
        let Some(p) = db.all_powers().find(|p| p.ident() == summon) else {
            println!("    {summon}: absent on this fork");
            continue;
        };
        let entity = p
            .extra
            .get("summon")
            .and_then(|s| s.get("entity"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        let counts: Vec<String> = p
            .atoms
            .iter()
            .filter(|a| a.effect_type == Some(EffectType::EntCreate))
            .map(|a| {
                format!(
                    "mag={:?} scale={:?} special={:?}",
                    a.magnitude, a.scale, a.special_case
                )
            })
            .collect();
        println!("    {summon} -> {entity}: {}", counts.join(", "));
    }
}

fn mm_headline(db: &PowerDatabase, dataset: DatasetId) {
    let mut state = coh_data::CharacterState::empty(dataset);
    state.level = LEVEL as u8;
    state.archetype.id = Some("mastermind".into());
    state.primary.id = Some("mastermind/mercenaries".into());
    for name in [
        "Soldiers",
        "Spec_Ops",
        "Medic",
        "Commando",
        "Equip_Mercenary",
        "Tactical_Upgrade",
        "Serum",
    ] {
        if !db.all_powers().any(|p| p.ident() == name) {
            continue;
        }
        let mut pick = SelectedPower::picked(name, "mastermind/mercenaries", 50);
        pick.is_active = true;
        state.primary.powers.push(pick);
    }
    state.secondary.id = Some("mastermind/force-field".into());
    state.secondary.powers = vec![SelectedPower::picked(
        "Personal_Force_Field",
        "mastermind/force-field",
        50,
    )];
    let faults = coh_data::granted_powers::sync_granted_powers(&mut state, db);
    assert!(faults.is_empty(), "grant reconcile faults: {faults:?}");
    let g = coh_math::recalculate(&state, db).bonuses;
    println!(
        "  MM HEADLINE (merc primary + PFF passive, Serum active): def M/R/A {:.1}/{:.1}/{:.1}, res S/L {:.1}/{:.1}%, damage buff +{:.1}%, to-hit +{:.1}%",
        g.defense_melee, g.defense_ranged, g.defense_aoe,
        g.resistance_smashing, g.resistance_lethal,
        g.damage, g.to_hit
    );
    if !g.errors.is_empty() {
        println!("  MM HEADLINE errors: {:?}", g.errors);
    }
}

/// The roster damage headline: each damage henchman's best single-ability sustained proxy
/// (per-activation damage / recharge) times the count the build fields, summed. A spam-attack
/// proxy, NOT a rotation model — the label the mock must carry. Counts at L50: Soldiers fields
/// 2 Soldiers + 1 Medic (the power's help: "The third Soldier you gain will be a Medic"),
/// Spec Ops 2, Commando 1. EVERY damage-dealing henchman counts, the Medic included: it heals
/// but also fires SMG_Burst (the same 2.25 dmg/s proxy as a Soldier's Quick Burst). Owner ruled
/// 2026-11-17: count all damage; the earlier "Medic deals no damage" exclusion was false.
fn roster_headline(db: &PowerDatabase) {
    let mut total = 0.0;
    for (entity_name, count, upgrades) in [
        ("MastermindPets_Soldier", 2.0, &["Equip_Mercenary", "Tactical_Upgrade"][..]),
        ("MastermindPets_Spec_Ops", 2.0, &["Equip_Mercenary", "Tactical_Upgrade"][..]),
        ("MastermindPets_Commando", 1.0, &["Equip_Mercenary", "Tactical_Upgrade"][..]),
        ("MastermindPets_Medic", 1.0, &["Equip_Mercenary", "Tactical_Upgrade"][..]),
    ] {
        let Some(entity) = mastermind_henchman(db, entity_name) else {
            continue;
        };
        let class = entity.get("characterClass").and_then(Value::as_str).unwrap_or("");
        let abilities = active_entity_abilities(entity, &|name| upgrades.contains(&name));
        let mut best = 0.0f64;
        let mut best_name = String::new();
        for a in abilities {
            let Some(components) = a.get("damage").and_then(Value::as_array).filter(|c| !c.is_empty()) else {
                continue;
            };
            let total_dmg: f64 = components
                .iter()
                .map(|c| {
                    resolve(
                        c.get("scale").and_then(Value::as_f64).unwrap_or(0.0),
                        c.get("table").and_then(Value::as_str),
                        class,
                        db,
                    )
                    .abs()
                })
                .sum();
            let cycle = a
                .get("recharge")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(a.get("castTime").and_then(Value::as_f64).unwrap_or(0.0));
            if cycle > 0.0 && total_dmg / cycle > best {
                best = total_dmg / cycle;
                best_name = a.get("displayName").and_then(Value::as_str).unwrap_or("?").to_string();
            }
        }
        println!("    {entity_name} x{count}: best proxy {best:.2} dmg/s ({best_name}) -> {:.2} dmg/s", best * count);
        total += best * count;
    }
    println!("  ROSTER DAMAGE HEADLINE (fully upgraded, spam proxy, no buffs): {total:.2} dmg/s");
}

fn main() {
    for dataset in FORKS {
        let db = load(dataset);
        println!("=== {dataset:?} — henchmen at L{LEVEL} (pet class tables; level offset unmodeled) ===");
        for entity_name in ["MastermindPets_Soldier", "MastermindPets_Spec_Ops", "MastermindPets_Commando"] {
            let Some(entity) = mastermind_henchman(&db, entity_name) else {
                println!("  {entity_name}: absent on this fork");
                continue;
            };
            let class = entity.get("characterClass").and_then(Value::as_str).unwrap_or("");
            println!("  --- {entity_name} (class {class}) ---");
            print_loadout(entity, class, &db, "base", &[]);
            print_loadout(entity, class, &db, "+Equip Mercenary", &["Equip_Mercenary"]);
            print_loadout(entity, class, &db, "+Equip +Tactical Upgrade", &["Equip_Mercenary", "Tactical_Upgrade"]);
        }
        roster(&db);
        println!("  ROSTER (L50, per Soldiers help: third Soldier is a Medic; every damage henchman counts):");
        roster_headline(&db);
        mm_headline(&db, dataset);
    }
}
