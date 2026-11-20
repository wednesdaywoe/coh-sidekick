//! The henchman roster a Mastermind build fields, and each henchman's OWN stats — a read-only
//! projection for the planner's main-tab damage meter and pet-tab detail (worklog henchman-panel).
//!
//! Three decisions live here, each measured rather than assumed:
//!
//! * **The roster comes from the summon powers' `EntCreate` atoms**, not the summon bag. The bag
//!   collapses a gated multi-entity summon to one entity (`extractSummon` reads base templates
//!   only), so the Soldiers Medic — the third slot, gated `combatlevel>17` — never reached it.
//!   Each `EntCreate` atom now carries its `entity_def` (the additive wire field, ENT-10) and its
//!   `requires_expression`, so the roster names every pet the build fields at its level and counts
//!   it by the atom's magnitude. The level gates read `combatlevel source> N >`, resolved through
//!   a private `LevelContext` that binds the source's combat level to the build level.
//!
//! * **Stats resolve at the pet's OWN class** ([`TableScope::Pet`], the ENT-10 reading), at the
//!   build's level. The henchman level offset is NOT in the export (measured absent: the entity
//!   defs carry their own 1–55 range, no master-relative offset), so the numbers are labeled
//!   "pet-class tables" and no offset is guessed.
//!
//! * **The damage headline counts EVERY damage-dealing henchman, the Medic included.** Owner ruled
//!   2026-11-17: the Medic heals but also fires `SMG_Burst` (the same proxy as a Soldier's Quick
//!   Burst), so excluding it undercounted real henchman damage. The headline is a labeled
//!   best-attack spam proxy — per-activation damage over the ability's recharge, times the count
//!   fielded — NOT a simulated AI rotation; a rotation model is a successor decision.
//!
//! [`summoned_entity_chain`](crate::granted::summoned_entity_chain) stays untouched: the
//! henchman-owner-buffs discipline keeps damage/proc/display walks from folding henchman attacks
//! into the summoner. This module is a separate read surface, never a fold into the caster.

use crate::expr::{eval_bool, EvalContext, EvalError, Value};
use crate::granted::{active_entity_abilities, mastermind_henchman};
use crate::scaled::resolve_scaled_effect_for;
use crate::totals::CalcError;
use coh_data::{
    Aspect, AtomicEffect, CharacterState, EffectType, Power, PowerDatabase, TableScope, ToWho,
};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// The basis label the damage meter carries: a single-ability spam proxy, not a rotation model.
pub const DAMAGE_BASIS: &str = "best-attack spam proxy";

/// Answers the summon level gates, which the corpus spells `combatlevel source> N >` — the
/// source's combat level compared against N. The expression VM classifies `combatlevel` as a
/// bareword reader, so it pushes the attribute NAME here and `source>` consumes it as the
/// attribute to read; that is the one form these gates take, and a build always knows its level.
/// Anything else is Indeterminate (the gate is not a level gate this projection answers).
struct LevelContext {
    level: f64,
}

impl EvalContext for LevelContext {
    fn resolve(&self, reader: &str, operands: &[Value]) -> Result<Value, EvalError> {
        match reader {
            "combatlevel" => Ok(Value::Symbol("combatlevel".into())),
            "source>" | "Source>" => match operands.first() {
                Some(Value::Symbol(s)) if s.eq_ignore_ascii_case("combatlevel") => {
                    Ok(Value::Number(self.level))
                }
                _ => Err(EvalError::Indeterminate(reader.into())),
            },
            _ => Err(EvalError::Indeterminate(reader.into())),
        }
    }
}

/// One henchman entity the build fields, and how many of it.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterEntry {
    pub entity: String,
    pub count: f64,
}

/// One damage component of an attack: a damage type and its per-hit magnitude at the pet class.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageComponent {
    pub damage_type: String,
    pub per_hit: f64,
}

/// One attack ability, valued as a sustained spam proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct AttackRow {
    pub display_name: String,
    pub internal_name: String,
    pub components: Vec<DamageComponent>,
    pub damage_per_activation: f64,
    pub recharge: f64,
    pub cast_time: f64,
    /// `damage_per_activation / max(recharge, cast_time)`; 0 when the ability has no cycle.
    pub proxy_dps: f64,
}

/// A henchman's defensive stats, resolved from its abilities' self-effects at the pet class.
/// Resistance/defense are fractions (0.261 = 26.1%); mez protection/resistance are magnitudes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Survivability {
    pub resistance: BTreeMap<String, f64>,
    pub defense: BTreeMap<String, f64>,
    pub mez_protection: BTreeMap<String, f64>,
    pub mez_resistance: BTreeMap<String, f64>,
}

/// One henchman's own stats as the build has it (upgrade-aware), with its roster count.
#[derive(Debug, Clone, PartialEq)]
pub struct PetDetail {
    pub entity: String,
    pub class: String,
    pub count: f64,
    pub survivability: Survivability,
    pub attacks: Vec<AttackRow>,
    pub best_attack: String,
    pub best_proxy: f64,
}

/// The opt-in buff a build's pet-only buff powers (Serum) put on a henchman, resolved at the pet
/// class. Per-type fractions: resistance adds to the pet's own, damage scales the matching type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuffDelta {
    pub resistance: BTreeMap<String, f64>,
    pub damage: BTreeMap<String, f64>,
    pub to_hit: f64,
    pub recovery: f64,
}

/// The whole projection: the roster, each unique henchman's detail, and the damage headline.
#[derive(Debug, Clone, PartialEq)]
pub struct HenchmanProjection {
    pub roster: Vec<RosterEntry>,
    pub pets: Vec<PetDetail>,
    /// Sum of `count × best_proxy` over every damage-dealing henchman (Medic included).
    pub headline_dps: f64,
    pub errors: Vec<CalcError>,
}

/// The henchman roster the build fields, from its summon powers' `EntCreate` atoms.
///
/// Each atom names its entity (`entity_def`) and gates on the build level; an atom whose gate
/// fails (or cannot be resolved) is not fielded. Only commandable henchman entities count — a
/// pseudo-pet shell the summon also creates is not a henchman. Counts sum the atom magnitudes
/// (an `EntCreate` that states no magnitude summons one).
pub fn roster(db: &PowerDatabase, state: &CharacterState) -> Vec<RosterEntry> {
    let ctx = LevelContext {
        level: f64::from(state.level),
    };
    let mut counts: BTreeMap<String, f64> = BTreeMap::new();
    for pick in state.picked_powers() {
        if let Some(power) = db.find_power(&pick.powerset, &pick.internal_name) {
            scan_entcreate(db, &power.atoms, &ctx, &mut counts);
        }
    }
    into_entries(counts)
}

/// The henchmen ONE summon power fields at `level` — the Info panel's per-power reading, as
/// opposed to [`roster`]'s whole-build one. Same gate and entity rules, one power's atoms.
pub fn power_roster(db: &PowerDatabase, power: &Power, level: u8) -> Vec<RosterEntry> {
    let ctx = LevelContext {
        level: f64::from(level),
    };
    let mut counts: BTreeMap<String, f64> = BTreeMap::new();
    scan_entcreate(db, &power.atoms, &ctx, &mut counts);
    into_entries(counts)
}

/// One power's henchman projection: its roster, each pet's detail, and their damage headline.
pub fn project_power(
    db: &PowerDatabase,
    power: &Power,
    level: u8,
    holds: &dyn Fn(&str) -> bool,
) -> HenchmanProjection {
    let roster = power_roster(db, power, level);
    let mut errors = Vec::new();
    let mut pets = Vec::new();
    let mut headline_dps = 0.0;
    for entry in &roster {
        if let Some(detail) = pet_detail(
            db,
            &entry.entity,
            i32::from(level),
            entry.count,
            holds,
            &mut errors,
        ) {
            headline_dps += detail.best_proxy * entry.count;
            pets.push(detail);
        }
    }
    HenchmanProjection {
        roster,
        pets,
        headline_dps,
        errors,
    }
}

/// Fold one power's `EntCreate` atoms into the roster counts: a named henchman entity whose
/// level gate passes, counted by its magnitude (an atom stating none summons one).
fn scan_entcreate(
    db: &PowerDatabase,
    atoms: &[AtomicEffect],
    ctx: &LevelContext,
    counts: &mut BTreeMap<String, f64>,
) {
    for atom in atoms {
        if atom.effect_type != Some(EffectType::EntCreate) {
            continue;
        }
        let Some(entity) = atom.entity_def.as_deref() else {
            continue;
        };
        if mastermind_henchman(db, entity).is_none() {
            continue;
        }
        if let Some(gate) = &atom.requires_expression {
            if !matches!(eval_bool(gate, ctx), Ok(true)) {
                continue;
            }
        }
        *counts.entry(entity.to_string()).or_insert(0.0) += atom.magnitude.unwrap_or(1.0);
    }
}

fn into_entries(counts: BTreeMap<String, f64>) -> Vec<RosterEntry> {
    counts
        .into_iter()
        .map(|(entity, count)| RosterEntry { entity, count })
        .collect()
}

/// One henchman's own stats at `level`, with the upgrade tiers the build holds (`holds` names a
/// granter power). `None` when the entity is not a Mastermind henchman on this fork.
pub fn pet_detail(
    db: &PowerDatabase,
    entity_name: &str,
    level: i32,
    count: f64,
    holds: &dyn Fn(&str) -> bool,
    errors: &mut Vec<CalcError>,
) -> Option<PetDetail> {
    let entity = mastermind_henchman(db, entity_name)?;
    let class = entity
        .get("characterClass")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let scope = TableScope::Pet(&class);
    let mut survivability = Survivability::default();
    let mut attacks = Vec::new();
    for ability in active_entity_abilities(entity, holds) {
        if let Some(effects) = ability.get("effects").and_then(Json::as_array) {
            for effect in effects {
                accumulate_self_effect(&mut survivability, effect, scope, level, db, errors);
            }
        }
        let Some(components) = ability
            .get("damage")
            .and_then(Json::as_array)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let mut parts = Vec::new();
        let mut total = 0.0;
        for c in components {
            let scale = c.get("scale").and_then(Json::as_f64).unwrap_or(0.0);
            let table = c.get("table").and_then(Json::as_str);
            let damage_type = c
                .get("damageType")
                .and_then(Json::as_str)
                .unwrap_or("?")
                .to_string();
            // CoH damage tables store negative magnitudes (damage is a negative HP delta).
            let per_hit = resolve_scaled_effect_for(scale, table, scope, level, db, errors).abs();
            total += per_hit;
            parts.push(DamageComponent {
                damage_type,
                per_hit,
            });
        }
        let recharge = ability
            .get("recharge")
            .and_then(Json::as_f64)
            .unwrap_or(0.0);
        let cast_time = ability
            .get("castTime")
            .and_then(Json::as_f64)
            .unwrap_or(0.0);
        let cycle = recharge.max(cast_time);
        attacks.push(AttackRow {
            display_name: ability
                .get("displayName")
                .and_then(Json::as_str)
                .unwrap_or("?")
                .to_string(),
            internal_name: ability
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("?")
                .to_string(),
            components: parts,
            damage_per_activation: total,
            recharge,
            cast_time,
            proxy_dps: if cycle > 0.0 { total / cycle } else { 0.0 },
        });
    }
    let (best_attack, best_proxy) = attacks
        .iter()
        .max_by(|a, b| a.proxy_dps.total_cmp(&b.proxy_dps))
        .map(|a| (a.display_name.clone(), a.proxy_dps))
        .unwrap_or_default();
    Some(PetDetail {
        entity: entity_name.to_string(),
        class,
        count,
        survivability,
        attacks,
        best_attack,
        best_proxy,
    })
}

/// Fold one self-effect (`SelfResistance`, `SelfDefense`, `SelfMezProtection`, `SelfMezResistance`)
/// into the survivability maps, keyed by the effect's stated types and resolved at the pet class.
fn accumulate_self_effect(
    out: &mut Survivability,
    effect: &Json,
    scope: TableScope<'_>,
    level: i32,
    db: &PowerDatabase,
    errors: &mut Vec<CalcError>,
) {
    let ty = effect.get("type").and_then(Json::as_str).unwrap_or("");
    let (map, types_key): (&mut BTreeMap<String, f64>, &str) = match ty {
        "SelfResistance" => (&mut out.resistance, "resistanceTypes"),
        "SelfDefense" => (&mut out.defense, "defenseTypes"),
        "SelfMezProtection" => (&mut out.mez_protection, "mezTypes"),
        "SelfMezResistance" => (&mut out.mez_resistance, "mezTypes"),
        _ => return,
    };
    let scale = effect.get("scale").and_then(Json::as_f64).unwrap_or(0.0);
    let table = effect.get("table").and_then(Json::as_str);
    let value = resolve_scaled_effect_for(scale, table, scope, level, db, errors).abs();
    let types: Vec<String> = effect
        .get(types_key)
        .and_then(Json::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Json::as_str)
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default();
    if types.is_empty() {
        map.entry(String::new()).or_insert(0.0);
        *map.get_mut("").unwrap() += value;
    } else {
        for t in types {
            *map.entry(t).or_insert(0.0) += value;
        }
    }
}

/// The buff a build's active pet-only buff powers put on a henchman of `pet_class`, resolved at
/// that class. Reads the `notOnCaster`-stamped `TargetAndPets` atoms (Serum's family) — the same
/// stamp that keeps them off the Mastermind's own totals — and resolves their `|scale| × table`
/// the way the engine's buff appliers do. Opt-in: this never moves the headline.
pub fn buff_delta(
    db: &PowerDatabase,
    state: &CharacterState,
    pet_class: &str,
    level: i32,
    errors: &mut Vec<CalcError>,
) -> BuffDelta {
    let scope = TableScope::Pet(pet_class);
    let mut delta = BuffDelta::default();
    for pick in state.picked_powers().filter(|p| p.is_active) {
        let Some(power) = db.find_power(&pick.powerset, &pick.internal_name) else {
            continue;
        };
        for atom in &power.atoms {
            if atom.not_on_caster != Some(true) || atom.to_who != Some(ToWho::TargetAndPets) {
                continue;
            }
            let scale = atom.scale.unwrap_or(0.0).abs();
            let value = resolve_scaled_effect_for(
                scale,
                atom.modifier_table.as_deref(),
                scope,
                level,
                db,
                errors,
            );
            match atom.effect_type {
                Some(EffectType::Resistance) if atom.aspect == Some(Aspect::Res) => {
                    if let Some(sub) = atom.sub_type {
                        *delta
                            .resistance
                            .entry(sub.as_wire().to_lowercase())
                            .or_insert(0.0) += value;
                    }
                }
                Some(EffectType::DamageBuff) if atom.aspect == Some(Aspect::Str) => {
                    if let Some(sub) = atom.sub_type {
                        *delta
                            .damage
                            .entry(sub.as_wire().to_lowercase())
                            .or_insert(0.0) += value;
                    }
                }
                Some(EffectType::ToHit) => delta.to_hit += value,
                Some(EffectType::Recovery) => delta.recovery += value,
                _ => {}
            }
        }
    }
    delta
}

/// A pet detail with the opt-in buff applied: resistance added to the pet's own, each attack's
/// damage scaled by the matching type's buff. The base detail is left untouched.
pub fn buffed_pet(base: &PetDetail, delta: &BuffDelta) -> PetDetail {
    let mut out = base.clone();
    for (t, v) in &delta.resistance {
        *out.survivability.resistance.entry(t.clone()).or_insert(0.0) += v;
    }
    for attack in &mut out.attacks {
        let mut total = 0.0;
        for c in &mut attack.components {
            let buff = delta
                .damage
                .get(&c.damage_type.to_lowercase())
                .copied()
                .unwrap_or(0.0);
            c.per_hit *= 1.0 + buff;
            total += c.per_hit;
        }
        attack.damage_per_activation = total;
        let cycle = attack.recharge.max(attack.cast_time);
        attack.proxy_dps = if cycle > 0.0 { total / cycle } else { 0.0 };
    }
    let (best_attack, best_proxy) = out
        .attacks
        .iter()
        .max_by(|a, b| a.proxy_dps.total_cmp(&b.proxy_dps))
        .map(|a| (a.display_name.clone(), a.proxy_dps))
        .unwrap_or_default();
    out.best_attack = best_attack;
    out.best_proxy = best_proxy;
    out
}

/// Project the whole henchman surface for a build: roster, per-pet detail (upgrade-aware via
/// `holds`), and the damage headline counting every damage-dealing henchman.
pub fn project(
    db: &PowerDatabase,
    state: &CharacterState,
    holds: &dyn Fn(&str) -> bool,
) -> HenchmanProjection {
    let level = i32::from(state.level);
    let roster = roster(db, state);
    let mut errors = Vec::new();
    let mut pets = Vec::new();
    let mut headline_dps = 0.0;
    for entry in &roster {
        if let Some(detail) = pet_detail(db, &entry.entity, level, entry.count, holds, &mut errors)
        {
            headline_dps += detail.best_proxy * entry.count;
            pets.push(detail);
        }
    }
    HenchmanProjection {
        roster,
        pets,
        headline_dps,
        errors,
    }
}
